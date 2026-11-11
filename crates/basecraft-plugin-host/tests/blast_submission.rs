//! Public runner boundary tests. A pinned bundled mock replaces HTTP entirely.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
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
import json, os, time
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin('basecraft.blast','0.1.0')
@plugin.context_operation('submit')
def submit(snapshot,params,context):
    plan=json.loads(Path(context['inputs']['queries'][0]).read_text())
    mode=plan['records'][0]['name']
    receipt=dict(schema=1,status='accepted',rid='ABCD1234567',url='https://blast.ncbi.nlm.nih.gov/Blast.cgi?CMD=Get&RID=ABCD1234567&FORMAT_TYPE=HTML',query_hash=plan['query_hash'],approval_hash=plan['approval_hash'],program=plan['program'],database=plan['database'],submitted_residues=plan['submitted_residues'],plan={k:v for k,v in plan.items() if k!='fasta'},request_fields={},submitted_at='synthetic',message='Submitted (mock)')
    output=Path(context['outputs']['receipt'])
    output.write_text(json.dumps(receipt))
    Path(context['progress_file']).write_text('accepted')
    if mode=='cancel':
        until=time.monotonic()+5
        while not Path(context['cancel_file']).exists() and time.monotonic()<until: time.sleep(.01)
    if mode=='crash': os._exit(17)
    if mode=='malformed': output.write_text('{bad receipt'); os._exit(18)
    if mode=='bad_identity': receipt['query_hash']='changed'; output.write_text(json.dumps(receipt))
    return dict(ops=[],data=receipt,artifacts=[dict(role='receipt',path='.')])
@plugin.context_operation('receipt')
def receipt(snapshot,params,context): return dict(ops=[],data=None)
plugin.run()
";

fn registry(root: &Path) -> Registry {
    let dir = root.join("bundle/blast");
    std::fs::create_dir_all(&dir).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/blast");
    for file in ["plugin.toml", "operations.json"] {
        std::fs::copy(source.join(file), dir.join(file)).unwrap();
    }
    std::fs::write(dir.join("blast_plugin.py"), MOCK).unwrap();
    let config = RegistryConfig {
        bundled: root.join("bundle"),
        packages: root.join("packages"),
        index: root.join("plugins.json"),
    };
    Registry::load(&config).unwrap()
}
fn request(root: &Path, mode: &str, destination: bool) -> OperationRequest {
    std::fs::write(
        root.join("source.fa"),
        format!(">{mode}\nACGT\n>second\nNNNN\n"),
    )
    .unwrap();
    serde_json::from_value(json!({"operation":"basecraft.blast/submit@1","inputs":{"queries":[{"kind":"file","path":"source.fa"}]},"parameters":{"alphabet":"nucleotide","database":"nt","contact_email":"test@example.org"},"destinations":if destination {json!({"receipt":"export.json"})} else {json!({})}})).unwrap()
}
fn wait(jobs: &JobManager, id: &str) -> OperationOutcome {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let outcome = jobs.get(id).unwrap();
        if outcome.state.terminal() {
            return outcome;
        }
        assert!(
            Instant::now() < deadline,
            "job did not terminate: {outcome:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn wait_marker(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(90);
    while !path.exists() {
        assert!(Instant::now() < deadline, "no marker at {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn start(root: &Path, mode: &str, destination: bool) -> (JobManager, String, PathBuf) {
    let registry = registry(root);
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(
            &registry,
            request(root, mode, destination),
            SessionPolicy::new(root, true).unwrap(),
            "mock public caller",
        )
        .unwrap();
    let evidence = PathBuf::from(accepted.evidence.unwrap());
    (jobs, accepted.run_id, evidence)
}

#[test]
fn blast_accepted_then_cancel_is_partial_repair_never_executes_again() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let (jobs, id, evidence) = start(temp.path(), "cancel", false);
    wait_marker(&evidence.join("progress"));
    jobs.cancel(&id).unwrap();
    let result = wait(&jobs, &id);
    assert_eq!(result.state, RuntimeState::Partial, "{result:?}");
    assert_eq!(result.data.as_ref().unwrap()["rid"], "ABCD1234567");
    let repaired = jobs.repair(&id).unwrap();
    assert_eq!(repaired.state, RuntimeState::Completed, "{repaired:?}");
    assert_eq!(repaired.data, result.data);
    assert_eq!(
        basecraft_ops::provenance::verify(Path::new(&repaired.artifacts[0].path)).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("source.fa")).unwrap(),
        ">cancel\nACGT\n>second\nNNNN\n"
    );
}
#[test]
fn blast_local_publication_failure_retains_rid_and_repairs_without_execution() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let (jobs, id, evidence) = start(temp.path(), "cancel", true);
    wait_marker(&evidence.join("progress"));
    // A competing destination appears after the host accepted the request.
    std::fs::write(temp.path().join("export.json"), "unrelated").unwrap();
    // This mock waits up to five seconds before returning an accepted response.
    let result = wait(&jobs, &id);
    assert_eq!(result.state, RuntimeState::Partial, "{result:?}");
    assert_eq!(result.data.as_ref().unwrap()["rid"], "ABCD1234567");
    std::fs::remove_file(temp.path().join("export.json")).unwrap();
    assert_eq!(jobs.repair(&id).unwrap().state, RuntimeState::Completed);
}
#[test]
fn blast_crash_after_receipt_recovers_while_malformed_identity_does_not() {
    crate::isolated_profile::isolate!();
    for mode in ["crash", "malformed", "bad_identity"] {
        let temp = tempfile::tempdir().unwrap();
        let (jobs, id, evidence) = start(temp.path(), mode, false);
        let outcome = wait(&jobs, &id);
        if mode == "crash" {
            assert!(
                matches!(
                    outcome.state,
                    RuntimeState::Completed | RuntimeState::Partial
                ),
                "{outcome:?}"
            );
            assert_eq!(outcome.data.as_ref().unwrap()["rid"], "ABCD1234567");
        } else {
            assert_eq!(outcome.state, RuntimeState::Failed, "{outcome:?}");
            assert!(outcome.data.is_none());
            assert!(evidence.join("query-preparation.json").exists());
        }
    }
}
#[test]
fn blast_oversize_rejected_before_spawn_and_changed_approval_rejected() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let registry = registry(root);
    let mut request = request(root, "normal", false);
    std::fs::write(
        root.join("source.fa"),
        format!(">big\n{}\n", "A".repeat(1_000_001)),
    )
    .unwrap();
    let mut jobs = JobManager::default();
    assert!(
        jobs.submit(
            &registry,
            request.clone(),
            SessionPolicy::new(root, true).unwrap(),
            "preflight"
        )
        .unwrap_err()
        .to_string()
        .contains("truncate=true")
    );
    request.parameters["truncate"] = json!(true);
    request.parameters["approval_hash"] = json!("old");
    assert!(
        jobs.submit(
            &registry,
            request,
            SessionPolicy::new(root, true).unwrap(),
            "preflight"
        )
        .unwrap_err()
        .to_string()
        .contains("source or settings changed")
    );
}
#[test]
fn blast_shutdown_retains_accepted_receipt_and_joins_bounded_worker() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let (jobs, _id, evidence) = start(temp.path(), "cancel", false);
    wait_marker(&evidence.join("progress"));
    let start = Instant::now();
    drop(jobs);
    assert!(start.elapsed() < Duration::from_secs(10));
    let runtime: Value =
        serde_json::from_slice(&std::fs::read(evidence.join("runtime.json")).unwrap()).unwrap();
    assert_eq!(runtime["state"], "partial");
    assert_eq!(runtime["data"]["rid"], "ABCD1234567");
    assert!(evidence.join("validated.json").exists());
}
