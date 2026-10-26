//! Installed external-package conformance, without application dispatch on its ID.
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{self, Registry, RegistryConfig},
};
use basecraft_protocol::operation::{OperationRequest, RuntimeState};
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

fn registry(root: &Path) -> Registry {
    let config = RegistryConfig {
        bundled: root.join("no-bundle"),
        packages: root.join("profile/plugins"),
        index: root.join("profile/index.json"),
    };
    registry::install(
        &config,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin"),
    )
    .unwrap();
    Registry::load(&config).unwrap()
}
fn request(mode: &str) -> OperationRequest {
    serde_json::from_value(json!({"operation":"org.example.registry-fixture/count@1","inputs":{"sources":[{"kind":"file","path":"a.txt"},{"kind":"file","path":"b.txt"}]},"parameters":{"mode":mode},"destinations":{"report":"report.json"}})).unwrap()
}
fn wait(manager: &JobManager, id: &str) -> basecraft_protocol::operation::OperationOutcome {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = manager.get(id).unwrap();
        if result.state.terminal() {
            return result;
        }
        assert!(Instant::now() < deadline, "job failed to terminate");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn installed_multi_input_job_uses_common_storage_and_frozen_identity() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("a.txt"), b"abc\n").unwrap();
    std::fs::write(temp.path().join("b.txt"), b"xy\n").unwrap();
    let registry = registry(temp.path());
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(
            &registry,
            request("normal"),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test client",
        )
        .unwrap();
    assert_eq!(accepted.state, RuntimeState::Queued);
    let result = wait(&jobs, &accepted.run_id);
    assert_eq!(result.state, RuntimeState::Completed, "{result:?}");
    assert_eq!(result.data, json!({"bytes":7}).into());
    assert_eq!(
        result.effective_parameters,
        json!({"include_newlines":true,"mode":"normal"})
    );
    let output = temp.path().join("report.json");
    let publication = basecraft_ops::provenance::inspect(&output).unwrap();
    assert_eq!(publication.inputs.len(), 2);
    assert!(
        basecraft_ops::provenance::verify(&output)
            .unwrap()
            .is_empty()
    );
    std::fs::write(temp.path().join("b.txt"), b"changed").unwrap();
    assert!(
        !basecraft_ops::provenance::verify(&output)
            .unwrap()
            .is_empty()
    );
    let execution: serde_json::Value = serde_json::from_slice(
        &std::fs::read(Path::new(result.evidence.as_ref().unwrap()).join("execution.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(execution["initiator"], "test client");
    assert_eq!(execution["executor"], "org.example.registry-fixture");
}
#[test]
fn invalid_data_does_not_publish_and_slow_job_cancels_without_extra_grants() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(temp.path().join(name), b"a").unwrap();
    }
    let registry = registry(temp.path());
    let mut jobs = JobManager::default();
    let invalid = jobs
        .submit(
            &registry,
            request("invalid_data"),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
        )
        .unwrap();
    assert_eq!(wait(&jobs, &invalid.run_id).state, RuntimeState::Failed);
    assert!(!temp.path().join("report.json").exists());
    let accepted = jobs
        .submit(
            &registry,
            request("slow"),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
        )
        .unwrap();
    jobs.cancel(&accepted.run_id).unwrap();
    assert_eq!(wait(&jobs, &accepted.run_id).state, RuntimeState::Cancelled);
    assert!(!temp.path().join("report.json").exists());
}
#[test]
fn denial_and_request_key_reuse_do_not_spawn_again() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(temp.path().join(name), b"a").unwrap();
    }
    let registry = registry(temp.path());
    let mut jobs = JobManager::default();
    assert!(
        jobs.submit(
            &registry,
            request("normal"),
            SessionPolicy::new(temp.path(), false).unwrap(),
            "read-only"
        )
        .is_err()
    );
    assert!(!temp.path().join(".basecraft").exists());
    let mut request = request("normal");
    request.request_key = Some("one".into());
    let first = jobs
        .submit(
            &registry,
            request.clone(),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
        )
        .unwrap();
    let repeated = jobs
        .submit(
            &registry,
            request.clone(),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
        )
        .unwrap();
    assert_eq!(first.run_id, repeated.run_id);
    request.parameters = json!({"mode":"slow"});
    assert!(
        jobs.submit(
            &registry,
            request,
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test"
        )
        .is_err()
    );
    assert_eq!(wait(&jobs, &first.run_id).state, RuntimeState::Completed);
}

#[test]
fn installed_snapshot_proposals_commit_in_owner_and_stale_jobs_preserve_edits() {
    use basecraft_ops::Session;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("sequence.fa");
    std::fs::write(&source, b">sequence\nACGTACGT\n").unwrap();
    let config = RegistryConfig {
        bundled: temp.path().join("no-bundle"),
        packages: temp.path().join("plugins"),
        index: temp.path().join("index.json"),
    };
    registry::install(
        &config,
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/registry-sequence-plugin"),
    )
    .unwrap();
    let registry = Registry::load(&config).unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&source).unwrap();
    let mut manager = JobManager::default();
    let request: OperationRequest = serde_json::from_value(json!({"operation":"org.example.registry-sequence/annotate@1",
        "inputs":{"sequence":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc),"revision":session.doc(doc).version()}]},"parameters":{}})).unwrap();
    let accepted = manager
        .submit_in_session(
            &registry,
            request,
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
            &mut session,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        manager.poll_proposals(&mut session);
        if manager.get(&accepted.run_id).unwrap().state.terminal() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let completed = manager.result(&accepted.run_id).unwrap();
    assert_eq!(completed.state, RuntimeState::Completed, "{completed:?}");
    assert_eq!(
        completed.proposals.unwrap().document,
        Some(doc.0.to_string())
    );
    assert_eq!(session.doc(doc).feature_count(), 1);
    session.undo(doc).unwrap();
    assert_eq!(session.doc(doc).feature_count(), 0);
    session.redo(doc).unwrap();
    assert_eq!(session.doc(doc).feature_count(), 1);
    let stale: OperationRequest = serde_json::from_value(json!({"operation":"org.example.registry-sequence/annotate@1",
        "inputs":{"sequence":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc),"revision":session.doc(doc).version()}]},"parameters":{"slow":true}})).unwrap();
    let accepted = manager
        .submit_in_session(
            &registry,
            stale,
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
            &mut session,
        )
        .unwrap();
    session
        .apply(
            doc,
            basecraft_protocol::Op::SeqReplace {
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        manager.poll_proposals(&mut session);
        if manager.get(&accepted.run_id).unwrap().state.terminal() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let failed = manager.result(&accepted.run_id).unwrap();
    assert_eq!(failed.state, RuntimeState::Failed);
    assert_eq!(session.doc(doc).feature_count(), 1);
    assert_eq!(session.doc(doc).read(0, 8), b"TCGTACGT");
    assert_eq!(std::fs::read(&source).unwrap(), b">sequence\nACGTACGT\n");
}
