//! Real registered runner + SDK adapter, with only HTTPS replaced by a fixture.
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::operation::{OperationOutcome, OperationRequest, RuntimeState};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MOCK: &str = r"
import os, time
from pathlib import Path
import zenodo_impl as z

def transport(method, url, token, context, **kwargs):
    if token != 'synthetic-runtime-secret': raise ValueError('runtime token missing')
    meta = z.metadata(PARAMETERS)
    if method == 'POST':
        if PARAMETERS['title'] == 'cancel':
            Path(context.progress_file).write_text('remote accepted')
        return dict(id=123,submitted=False,metadata=meta,files=[],links=dict(bucket='https://sandbox.zenodo.org/api/files/test'))
    if method == 'PUT':
        if PARAMETERS['title'] == 'cancel':
            deadline=time.monotonic()+10
            while not Path(context.cancel_file).exists() and time.monotonic()<deadline: time.sleep(.01)
            z.check_cancel(context)
        f=z.digest_file(kwargs['file'], context)
        return dict(key='source.fa',size=f['size'],checksum='md5:'+f['md5'])
    raise ValueError('unexpected request')

def draft(params, context):
    global PARAMETERS
    PARAMETERS=params
    return z.upload(params, context, transport)
z.plugin._ops['draft']=draft
z.plugin.run()
";
fn registry(root: &Path) -> Registry {
    let dir = root.join("bundle/zenodo");
    std::fs::create_dir_all(&dir).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/zenodo");
    for file in ["plugin.toml", "operations.json"] {
        std::fs::copy(source.join(file), dir.join(file)).unwrap();
    }
    std::fs::copy(source.join("zenodo_plugin.py"), dir.join("zenodo_impl.py")).unwrap();
    std::fs::write(dir.join("zenodo_plugin.py"), MOCK).unwrap();
    Registry::load(&RegistryConfig {
        bundled: root.join("bundle"),
        packages: root.join("packages"),
        index: root.join("plugins.json"),
    })
    .unwrap()
}
fn wait(jobs: &JobManager, id: &str) -> OperationOutcome {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let outcome = jobs.get(id).unwrap();
        if outcome.state.terminal() {
            return outcome;
        }
        assert!(Instant::now() < deadline, "{outcome:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn prepare(root: &Path, registry: &Registry, jobs: &mut JobManager) -> OperationOutcome {
    std::fs::write(root.join("source.fa"), b">synthetic\nACGT\n").unwrap();
    let request:OperationRequest=serde_json::from_value(json!({"operation":"basecraft.zenodo/prepare@1","inputs":{"files":[{"kind":"file","path":"source.fa"}]},"parameters":{}})).unwrap();
    let accepted = jobs
        .submit(
            registry,
            request,
            SessionPolicy::new(root, true).unwrap(),
            "test",
        )
        .unwrap();
    let outcome = wait(jobs, &accepted.run_id);
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
    outcome
}
fn start(root: &Path, cancel: bool, explicit: bool) -> (JobManager, String, PathBuf) {
    let registry = registry(root);
    let mut jobs = JobManager::default();
    let prepared = prepare(root, &registry, &mut jobs);
    let plan = prepared.data.as_ref().unwrap();
    // The selected snapshot stays the same even after an original-source edit.
    std::fs::write(root.join("source.fa"), b"later edits").unwrap();
    let mut policy = SessionPolicy::new(root, true).unwrap();
    if explicit {
        policy = policy
            .with_runtime_credential(
                registry.resolve("basecraft.zenodo/draft@1", None).unwrap(),
                "zenodo_access",
                "synthetic-runtime-secret".into(),
            )
            .unwrap();
    }
    assert!(!format!("{policy:?}").contains("synthetic-runtime-secret"));
    let request=serde_json::from_value(json!({"operation":"basecraft.zenodo/draft@1","inputs":{"files":[{"kind":"file","path":plan["files"][0]["snapshot"]}],"selection":[{"kind":"file","path":prepared.artifacts[0].path}]},"parameters":{"title":if cancel {"cancel"} else {"Synthetic dataset"},"description":"Fixture only","creators":"Example, A.","license":"cc-by-4.0"}})).unwrap();
    let accepted = jobs.submit(&registry, request, policy, "test").unwrap();
    (
        jobs,
        accepted.run_id,
        PathBuf::from(accepted.evidence.unwrap()),
    )
}
#[test]
fn zenodo_shared_runner_preserves_snapshots_and_never_records_token() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let (jobs, id, evidence) = start(temp.path(), false, true);
    let outcome = wait(&jobs, &id);
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
    assert_eq!(outcome.data.as_ref().unwrap()["status"], "draft_ready");
    let execution = std::fs::read_to_string(evidence.join("execution.json")).unwrap();
    assert!(!execution.contains("synthetic-runtime-secret"));
    assert!(
        !serde_json::to_string(&outcome)
            .unwrap()
            .contains("synthetic-runtime-secret")
    );
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(&outcome.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(receipt["draft_id"], 123);
    assert!(!receipt.to_string().contains("synthetic-runtime-secret"));
}
#[test]
fn zenodo_cancel_retains_remote_identity_and_repair_does_not_resubmit() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let (jobs, id, evidence) = start(temp.path(), true, true);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !evidence.join("progress").is_file() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    // Wait until the receipt proves the create response was retained.
    loop {
        if std::fs::read(evidence.join("pending/receipt"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|v| v["draft_id"] == 123)
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    jobs.cancel(&id).unwrap();
    let outcome = wait(&jobs, &id);
    assert_eq!(outcome.state, RuntimeState::Partial, "{outcome:?}");
    assert_eq!(outcome.data.as_ref().unwrap()["draft_id"], 123);
    let repaired = jobs.repair(&id).unwrap();
    assert_eq!(repaired.state, RuntimeState::Completed, "{repaired:?}");
    assert_eq!(repaired.data.as_ref().unwrap()["draft_id"], 123);
}

#[test]
fn zenodo_headless_ambient_provider_delivers_only_to_accepted_bundled_draft() {
    crate::isolated_profile::isolate!();
    if std::env::var_os("BASECRAFT_ZENODO_TEST_CHILD").is_none() {
        let output = crate::isolated_profile::relaunch(&crate::isolated_profile::test_name())
            .env("BASECRAFT_ZENODO_TEST_CHILD", "1")
            .env("BASECRAFT_ZENODO_TOKEN", "synthetic-runtime-secret")
            .output()
            .unwrap();
        crate::isolated_profile::assert_one_passed(&crate::isolated_profile::test_name(), &output);
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let (jobs, id, evidence) = start(temp.path(), false, false);
    let outcome = wait(&jobs, &id);
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
    assert_eq!(outcome.data.as_ref().unwrap()["status"], "draft_ready");
    assert!(
        !std::fs::read_to_string(evidence.join("execution.json"))
            .unwrap()
            .contains("synthetic-runtime-secret")
    );
}
