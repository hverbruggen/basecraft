//! Installed external-package conformance, without application dispatch on its ID.
use crate::isolated_profile;
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
    registry_retention(root, false)
}
fn registry_retention(root: &Path, retain: bool) -> Registry {
    registry_inputs(root, retain, None)
}
fn registry_inputs(root: &Path, retain: bool, inputs: Option<serde_json::Value>) -> Registry {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin");
    let fixture = if retain || inputs.is_some() {
        let package = root.join("retained-fixture");
        std::fs::create_dir(&package).unwrap();
        for name in ["plugin.toml", "plugin.py", "operations.json"] {
            std::fs::copy(fixture.join(name), package.join(name)).unwrap();
        }
        let mut contract: serde_json::Value =
            serde_json::from_slice(&std::fs::read(package.join("operations.json")).unwrap())
                .unwrap();
        if retain {
            contract["operations"][0]["inputs"]["sources"]["retain"] = json!(true);
        }
        if let Some(inputs) = inputs {
            contract["operations"][0]["inputs"] = inputs;
        }
        std::fs::write(
            package.join("operations.json"),
            serde_json::to_vec(&contract).unwrap(),
        )
        .unwrap();
        package
    } else {
        fixture
    };
    let config = RegistryConfig {
        bundled: root.join("no-bundle"),
        packages: root.join("profile/plugins"),
        index: root.join("profile/index.json"),
    };
    registry::install(&config, &fixture).unwrap();
    Registry::load(&config).unwrap()
}
fn request(mode: &str) -> OperationRequest {
    serde_json::from_value(json!({"operation":"org.example.registry-fixture/count@1","inputs":{"sources":[{"kind":"file","path":"a.txt"},{"kind":"file","path":"b.txt"}]},"parameters":{"mode":mode},"destinations":{"report":"report.json"}})).unwrap()
}
fn wait(manager: &JobManager, id: &str) -> basecraft_protocol::operation::OperationOutcome {
    let deadline = Instant::now() + Duration::from_secs(30);
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
    crate::isolated_profile::isolate!();
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
    let publication = basecraft_ops::records::inspect(&output, None).unwrap();
    assert_eq!(publication.inputs.len(), 2);
    assert_eq!(
        basecraft_ops::records::verify(&output, None).unwrap(),
        Vec::<String>::new()
    );
    std::fs::write(temp.path().join("b.txt"), b"changed").unwrap();
    assert_ne!(
        basecraft_ops::records::verify(&output, None).unwrap(),
        Vec::<String>::new()
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
fn registered_newick_inputs_keep_verifiable_identity_evidence() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin");
    let package = temp.path().join("package");
    std::fs::create_dir(&package).unwrap();
    for name in ["plugin.toml", "plugin.py", "operations.json"] {
        std::fs::copy(fixture.join(name), package.join(name)).unwrap();
    }
    let mut contract: serde_json::Value =
        serde_json::from_slice(&std::fs::read(package.join("operations.json")).unwrap()).unwrap();
    contract["operations"][0]["inputs"]["sources"]["view"] = json!("newick");
    std::fs::write(
        package.join("operations.json"),
        serde_json::to_vec(&contract).unwrap(),
    )
    .unwrap();
    let config = RegistryConfig {
        bundled: temp.path().join("no-bundle"),
        packages: temp.path().join("profile/plugins"),
        index: temp.path().join("profile/index.json"),
    };
    registry::install(&config, &package).unwrap();
    let registry = Registry::load(&config).unwrap();
    let original = b"(a:1,b:1);\n";
    for name in ["a.txt", "b.txt"] {
        std::fs::write(temp.path().join(name), original).unwrap();
    }
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(
            &registry,
            request("normal"),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "newick client",
        )
        .unwrap();
    let outcome = wait(&jobs, &accepted.run_id);
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
    assert_eq!(
        basecraft_ops::records::verify(&temp.path().join("report.json"), None).unwrap(),
        Vec::<String>::new()
    );
    for name in ["a.txt", "b.txt"] {
        assert_eq!(std::fs::read(temp.path().join(name)).unwrap(), original);
    }
}

#[test]
fn invalid_data_does_not_publish_and_slow_job_cancels_without_extra_grants() {
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
        "inputs":{"sequence":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc).unwrap(),"revision":session.doc(doc).unwrap().version()}]},"parameters":{}})).unwrap();
    let accepted = manager
        .submit_in_session(
            &registry,
            request,
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
            &mut session,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        manager.poll_session(&mut session);
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
    assert_eq!(session.doc(doc).unwrap().feature_count(), 1);
    session.undo(doc).unwrap();
    assert_eq!(session.doc(doc).unwrap().feature_count(), 0);
    session.redo(doc).unwrap();
    assert_eq!(session.doc(doc).unwrap().feature_count(), 1);
    let stale: OperationRequest = serde_json::from_value(json!({"operation":"org.example.registry-sequence/annotate@1",
        "inputs":{"sequence":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc).unwrap(),"revision":session.doc(doc).unwrap().version()}]},"parameters":{"slow":true}})).unwrap();
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
        .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        manager.poll_session(&mut session);
        if manager.get(&accepted.run_id).unwrap().state.terminal() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let failed = manager.result(&accepted.run_id).unwrap();
    assert_eq!(failed.state, RuntimeState::Failed);
    assert_eq!(session.doc(doc).unwrap().feature_count(), 1);
    assert_eq!(session.doc(doc).unwrap().read(0, 8), b"TCGTACGT");
    assert_eq!(std::fs::read(&source).unwrap(), b">sequence\nACGTACGT\n");
}

#[test]
fn hostile_processes_and_unallocated_outputs_never_publish_success() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(temp.path().join(name), b"a").unwrap();
    }
    let registry = registry(temp.path());
    let mut jobs = JobManager::default();
    for mode in ["crash", "noisy", "extra_output", "symlink_output"] {
        let accepted = jobs
            .submit(
                &registry,
                request(mode),
                SessionPolicy::new(temp.path(), true).unwrap(),
                "hostile fixture",
            )
            .unwrap();
        let result = wait(&jobs, &accepted.run_id);
        assert_eq!(result.state, RuntimeState::Failed, "{mode}: {result:?}");
        assert!(!temp.path().join("report.json").exists());
        assert!(Path::new(result.evidence.as_ref().unwrap()).is_dir());
    }
}

#[test]
fn uncooperative_running_process_is_cancelled_and_reaped() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(temp.path().join(name), b"a").unwrap();
    }
    let registry = registry(temp.path());
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(
            &registry,
            request("hang"),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while jobs.get(&accepted.run_id).unwrap().state == RuntimeState::Queued {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    // Give the fixture time to enter its uncooperative loop. Cancellation also
    // remains valid if slow initialization has not finished on this machine.
    std::thread::sleep(Duration::from_millis(250));
    jobs.cancel(&accepted.run_id).unwrap();
    assert_eq!(wait(&jobs, &accepted.run_id).state, RuntimeState::Cancelled);
    assert!(!temp.path().join("report.json").exists());
}

#[test]
fn submitted_views_remain_frozen_when_both_originals_change_during_execution() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("a.txt"), b"abc\n").unwrap();
    std::fs::write(temp.path().join("b.txt"), b"xy\n").unwrap();
    let registry = registry(temp.path());
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(
            &registry,
            request("delayed"),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
        )
        .unwrap();
    std::fs::write(temp.path().join("a.txt"), b"new bytes").unwrap();
    std::fs::write(temp.path().join("b.txt"), b"other bytes").unwrap();
    let outcome = wait(&jobs, &accepted.run_id);
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
    assert_eq!(outcome.data, Some(json!({"bytes":7})));
    assert_ne!(
        basecraft_ops::records::verify(&temp.path().join("report.json"), None).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn late_collision_preserves_foreign_bytes_and_repairs_without_reexecuting() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("a.txt"), b"abc\n").unwrap();
    std::fs::write(temp.path().join("b.txt"), b"xy\n").unwrap();
    let registry = registry(temp.path());
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(
            &registry,
            request("delayed"),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "test",
        )
        .unwrap();
    let destination = temp.path().join("report.json");
    std::fs::write(&destination, b"unrelated concurrent writer").unwrap();
    let partial = wait(&jobs, &accepted.run_id);
    assert_eq!(partial.state, RuntimeState::Partial, "{partial:?}");
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"unrelated concurrent writer"
    );
    let evidence = Path::new(partial.evidence.as_ref().unwrap()).join("execution.json");
    let execution = std::fs::read(&evidence).unwrap();
    // Remove only the test-owned collision to make its destination available.
    std::fs::remove_file(&destination).unwrap();
    let repaired = jobs.repair(&accepted.run_id).unwrap();
    assert_eq!(repaired.state, RuntimeState::Completed, "{repaired:?}");
    assert_eq!(repaired.run_id, accepted.run_id);
    assert_eq!(std::fs::read(&evidence).unwrap(), execution);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(destination).unwrap()).unwrap(),
        json!({"bytes":7})
    );
    assert!(
        jobs.repair(&accepted.run_id).is_err(),
        "completed job cannot be rerun as repair"
    );
}

#[test]
fn committed_proposals_stay_partial_and_repair_requires_recorded_session_activity() {
    crate::isolated_profile::isolate!();
    for missing_activity in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("sequence.fa");
        let original = b">sequence\nACGTACGT\n";
        std::fs::write(&source, original).unwrap();
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
        let mut session = basecraft_ops::Session::new(temp.path());
        let doc = session.open(&source).unwrap();
        let request: OperationRequest = serde_json::from_value(json!({
            "operation":"org.example.registry-sequence/annotate@1",
            "inputs":{"sequence":[{"kind":"document","doc":doc.0.to_string(),
                "generation":session.generation(doc).unwrap(),"revision":session.doc(doc).unwrap().version()}]},
            "parameters":{}
        }))
        .unwrap();
        let mut manager = JobManager::default();
        let accepted = manager
            .submit_in_session(
                &registry,
                request,
                SessionPolicy::new(temp.path(), true).unwrap(),
                "test",
                &mut session,
            )
            .unwrap();
        let run = Path::new(accepted.evidence.as_ref().unwrap());
        let snapshot = run.join("snapshot.arrow-pair");
        let retained_snapshot = std::fs::read(&snapshot).unwrap();
        if missing_activity {
            // The sealed in-memory plugin input survives; the later owner-thread
            // activity hash fails after the valid proposals have been applied.
            std::fs::remove_file(&snapshot).unwrap();
        } else {
            // ToolRun logging succeeds, but publication cannot create records/.
            std::fs::write(run.join("records"), b"injected publication obstruction").unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            manager.poll_session(&mut session);
            if manager.get(&accepted.run_id).unwrap().state.terminal() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let partial = manager.result(&accepted.run_id).unwrap();
        assert_eq!(partial.state, RuntimeState::Partial, "{partial:?}");
        assert_eq!(partial.artifacts, Vec::new());
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(run.join("runtime.json")).unwrap()).unwrap();
        assert_eq!(persisted["state"], "partial");
        assert_eq!(
            partial.errors[0].code,
            if missing_activity {
                basecraft_protocol::operation::OperationErrorCode::EvidenceIncomplete
            } else {
                basecraft_protocol::operation::OperationErrorCode::PublicationIncomplete
            }
        );
        assert_eq!(partial.proposals.as_ref().unwrap().applied, 1);
        assert_eq!(session.doc(doc).unwrap().feature_count(), 1);
        let activities = || {
            session
                .document_record(doc)
                .unwrap()
                .events
                .iter()
                .filter(|event| matches!(event.op, basecraft_protocol::Op::ToolRun { .. }))
                .count()
        };
        if missing_activity {
            assert_eq!(activities(), 0);
            std::fs::write(&snapshot, retained_snapshot).unwrap();
            let error = manager.repair(&accepted.run_id).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("publication-only repair cannot restore")
            );
            assert_eq!(
                manager.result(&accepted.run_id).unwrap().state,
                RuntimeState::Partial
            );
            assert_eq!(
                activities(),
                0,
                "repair must not pretend to restore the missing ToolRun"
            );
        } else {
            assert_eq!(activities(), 1);
            std::fs::remove_file(run.join("records")).unwrap();
            // A failed runtime receipt during repair must also leave an owned,
            // terminal Partial state, rather than getting stuck Finalizing.
            std::fs::remove_file(run.join("runtime.json")).unwrap();
            std::fs::create_dir(run.join("runtime.json")).unwrap();
            let still_partial = manager.repair(&accepted.run_id).unwrap();
            assert_eq!(still_partial.state, RuntimeState::Partial);
            assert_eq!(
                manager.get(&accepted.run_id).unwrap().state,
                RuntimeState::Partial
            );
            assert!(still_partial.errors.iter().any(|error| error.code
                == basecraft_protocol::operation::OperationErrorCode::EvidenceIncomplete));
            std::fs::remove_dir(run.join("runtime.json")).unwrap();
            let repaired = manager.repair(&accepted.run_id).unwrap();
            assert_eq!(repaired.state, RuntimeState::Completed, "{repaired:?}");
            assert_eq!(
                activities(),
                1,
                "publication repair must not repeat the activity"
            );
        }
        assert_eq!(
            session.doc(doc).unwrap().feature_count(),
            1,
            "repair must not reapply edits"
        );
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).unwrap().feature_count(), 0);
        assert_eq!(std::fs::read(source).unwrap(), original);
    }
}

/// Install the count fixture with its input view set to an aligned FASTA, so a
/// session alignment document can be bound to it.
fn alignment_registry(root: &Path) -> Registry {
    fasta_registry(root, "aligned_fasta")
}

fn fasta_registry(root: &Path, view: &str) -> Registry {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin");
    let package = root.join("package");
    std::fs::create_dir(&package).unwrap();
    for name in ["plugin.toml", "plugin.py", "operations.json"] {
        std::fs::copy(fixture.join(name), package.join(name)).unwrap();
    }
    let mut contract: serde_json::Value =
        serde_json::from_slice(&std::fs::read(package.join("operations.json")).unwrap()).unwrap();
    contract["operations"][0]["inputs"]["sources"]["view"] = json!(view);
    contract["operations"][0]["inputs"]["sources"]["min"] = json!(1);
    std::fs::write(
        package.join("operations.json"),
        serde_json::to_vec(&contract).unwrap(),
    )
    .unwrap();
    let config = RegistryConfig {
        bundled: root.join("no-bundle"),
        packages: root.join("profile/plugins"),
        index: root.join("profile/index.json"),
    };
    registry::install(&config, &package).unwrap();
    Registry::load(&config).unwrap()
}

fn run_in_session(
    manager: &mut JobManager,
    registry: &Registry,
    session: &mut basecraft_ops::Session,
    request: serde_json::Value,
) -> basecraft_protocol::operation::OperationOutcome {
    let accepted = manager
        .submit_in_session(
            registry,
            serde_json::from_value(request).unwrap(),
            SessionPolicy::new(session.root(), true).unwrap(),
            "test",
            session,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        manager.poll_session(session);
        let outcome = manager.get(&accepted.run_id).unwrap();
        if outcome.state.terminal() {
            return outcome;
        }
        assert!(Instant::now() < deadline, "job failed to terminate");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn document_inputs_read_the_session_revision_and_record_history() {
    use basecraft_ops::Session;
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let registry = alignment_registry(temp.path());
    let source = temp.path().join("input.afa");
    std::fs::write(&source, ">a\nAC-T\n>b\nACGT\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&source).unwrap();
    let mut manager = JobManager::default();
    let document = |session: &Session| json!([{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc).unwrap(),"revision":session.revision(doc).unwrap()}]);
    // A clean document is submitted as its saved file.
    let request = json!({"operation":"org.example.registry-fixture/count@1","inputs":{"sources":document(&session)},
            "parameters":{"mode":"normal"},"destinations":{"report":"clean.json"}});
    let clean = run_in_session(&mut manager, &registry, &mut session, request);
    assert_eq!(clean.state, RuntimeState::Completed, "{clean:?}");
    assert_eq!(
        basecraft_ops::records::verify(&temp.path().join("clean.json"), None).unwrap(),
        Vec::<String>::new()
    );
    // Unsaved edits are submitted as a generated snapshot; the file is untouched.
    session
        .apply(
            doc,
            basecraft_protocol::Op::AlnReplace {
                row: 0,
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    let request = json!({"operation":"org.example.registry-fixture/count@1","inputs":{"sources":document(&session)},
            "parameters":{"mode":"normal"},"destinations":{"report":"edited.json"}});
    let edited = run_in_session(&mut manager, &registry, &mut session, request);
    assert_eq!(edited.state, RuntimeState::Completed, "{edited:?}");
    let evidence = Path::new(edited.evidence.as_ref().unwrap());
    let input: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence.join("input.json")).unwrap()).unwrap();
    assert_eq!(input["state"], "generated");
    assert_eq!(input["transformation"], "canonical-fasta-v1");
    assert!(
        String::from_utf8(std::fs::read(evidence.join("submitted.fasta")).unwrap())
            .unwrap()
            .contains("TC-T")
    );
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        ">a\nAC-T\n>b\nACGT\n"
    );
    // The result carries the submitted revision and producing activity, too.
    let publication =
        basecraft_ops::records::inspect(&temp.path().join("edited.json"), None).unwrap();
    assert!(
        publication
            .events
            .iter()
            .any(|event| event.op.is_run(&edited.run_id))
    );
    assert!(
        publication
            .effective_edits
            .iter()
            .any(|edit| matches!(&edit.operation,
        basecraft_protocol::Op::AlnReplace { residues, .. } if residues == "T"))
    );
    // Both runs are in the document's history, linked by run ID.
    let record = session.document_record(doc).unwrap();
    let runs: Vec<_> = record
        .events
        .iter()
        .filter_map(|event| match &event.op {
            basecraft_protocol::Op::ToolRun {
                run: Some(link), ..
            } => Some(link.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(runs, [clean.run_id.as_str(), edited.run_id.as_str()]);
    let finished: Vec<_> = record
        .events
        .iter()
        .filter_map(|event| match &event.op {
            basecraft_protocol::Op::JobFinished { report } => {
                Some((report.id.as_str(), report.status.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        finished,
        [
            (
                clean.run_id.as_str(),
                basecraft_protocol::records::JobStatus::Completed
            ),
            (
                edited.run_id.as_str(),
                basecraft_protocol::records::JobStatus::Completed
            )
        ]
    );
    // The Tasks view lists registered runs and points at their own record.
    let tasks = basecraft_ops::records::jobs(true, &[temp.path().to_path_buf()]).unwrap();
    let listed = tasks["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == clean.run_id.as_str())
        .cloned()
        .unwrap();
    assert_eq!(listed["status"], "completed");
    assert!(Path::new(listed["runtime_record"].as_str().unwrap()).is_file());
    // A failed document-bound run is recorded as an attempt, without a result.
    let request = json!({"operation":"org.example.registry-fixture/count@1","inputs":{"sources":document(&session)},
            "parameters":{"mode":"fail"},"destinations":{"report":"failed.json"}});
    let failed = run_in_session(&mut manager, &registry, &mut session, request);
    assert_eq!(failed.state, RuntimeState::Failed, "{failed:?}");
    let record = session.document_record(doc).unwrap();
    assert!(record.events.iter().any(|event| matches!(&event.op,
        basecraft_protocol::Op::JobFinished { report }
            if report.id == failed.run_id
                && report.status == basecraft_protocol::records::JobStatus::Failed)));
    // A request pinned to an older revision is refused before anything runs.
    let stale = manager.submit_in_session(
        &registry,
        serde_json::from_value(json!({"operation":"org.example.registry-fixture/count@1",
            "inputs":{"sources":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc).unwrap(),"revision":0}]},
            "parameters":{"mode":"normal"},"destinations":{"report":"stale.json"}}))
        .unwrap(),
        SessionPolicy::new(temp.path(), true).unwrap(),
        "test",
        &mut session,
    );
    assert!(stale.unwrap_err().to_string().contains("stale_revision"));
    assert!(!temp.path().join("stale.json").exists());
}

#[test]
fn job_records_stay_with_the_data_when_the_private_folder_is_deleted() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    // A plugin run in a project subfolder, from a session with that project.
    let project = root.join("project");
    let results = project.join("results");
    std::fs::create_dir_all(&results).unwrap();
    std::fs::write(results.join("a.txt"), b"abc\n").unwrap();
    std::fs::write(results.join("b.txt"), b"xy\n").unwrap();
    let registry = registry(&root);
    let mut session = basecraft_ops::Session::new(&results).in_project(Some(
        basecraft_ops::project::Project::new(&project).unwrap(),
    ));
    let mut manager = JobManager::default();
    let plugin = run_in_session(
        &mut manager,
        &registry,
        &mut session,
        serde_json::to_value(request("normal")).unwrap(),
    );
    assert_eq!(plugin.state, RuntimeState::Completed, "{plugin:?}");
    // A Sanger stage on a loose trace outside any project.
    let loose = root.join("loose");
    std::fs::create_dir(&loose).unwrap();
    let trace = loose.join("trace.ab1");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &trace,
    )
    .unwrap();
    let stage = basecraft_ops::sanger_review::SangerRun::prepare(
        &loose.join("consensus"),
        "consensus",
        std::slice::from_ref(&trace),
        None,
    )
    .unwrap();
    let sanger = stage.directory().file_name().unwrap().to_owned();
    stage.failed("stopped by the test").unwrap();
    drop(stage);

    let state = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state;
    assert!(
        !state.join("jobs").exists(),
        "a job record went to the private folder"
    );
    if state.exists() {
        std::fs::remove_dir_all(&state).unwrap();
    }
    assert!(
        project
            .join(".basecraft/jobs")
            .join(&plugin.run_id)
            .is_dir()
    );
    assert!(loose.join(".basecraft/jobs").join(&sanger).is_dir());
    let listed = basecraft_ops::records::jobs(true, &[project, loose]).unwrap();
    let ids: std::collections::BTreeMap<_, _> = listed["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| (job["id"].as_str().unwrap(), job["status"].as_str().unwrap()))
        .collect();
    let sanger = sanger.to_string_lossy();
    let expected = std::collections::BTreeMap::from([
        (plugin.run_id.as_str(), "completed"),
        (sanger.as_ref(), "failed"),
    ]);
    assert_eq!(ids, expected, "{listed}");
}

#[test]
fn document_repair_waits_for_owner_history_and_does_not_repeat_activity() {
    use basecraft_ops::Session;
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let registry = alignment_registry(root.path());
    let source = root.path().join("input.afa");
    std::fs::write(&source, ">a\nACGT\n>b\nAC-T\n").unwrap();
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    let mut manager = JobManager::default();
    let accepted = manager.submit_in_session(&registry, serde_json::from_value(json!({
        "operation":"org.example.registry-fixture/count@1",
        "inputs":{"sources":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc).unwrap(),"revision":session.revision(doc).unwrap()}]},
        "parameters":{"mode":"delayed"},"destinations":{"report":"result.json"}
    })).unwrap(), SessionPolicy::new(root.path(), true).unwrap(), "test", &mut session).unwrap();
    let output = root.path().join("result.json");
    std::fs::write(&output, b"foreign").unwrap();
    assert_eq!(
        wait(&manager, &accepted.run_id).state,
        RuntimeState::Partial
    );
    std::fs::remove_file(&output).unwrap();
    assert_eq!(
        manager.repair(&accepted.run_id).unwrap().state,
        RuntimeState::Finalizing,
        "publication repair still owes owner-session finalisation"
    );
    manager.poll_session(&mut session);
    assert_eq!(
        manager.get(&accepted.run_id).unwrap().state,
        RuntimeState::Completed
    );
    manager.poll_session(&mut session);
    let record = session.document_record(doc).unwrap();
    assert_eq!(
        record
            .events
            .iter()
            .filter(|event| event.op.is_run(&accepted.run_id))
            .count(),
        1
    );
    assert_eq!(
        record
            .events
            .iter()
            .filter(|event| matches!(&event.op,
        basecraft_protocol::Op::JobFinished { report } if report.id == accepted.run_id
            && report.status == basecraft_protocol::records::JobStatus::Completed))
            .count(),
        1
    );
    let publication = basecraft_ops::records::inspect(&output, None).unwrap();
    assert!(
        publication
            .events
            .iter()
            .any(|event| event.op.is_run(&accepted.run_id))
    );
}

#[test]
fn canonical_alignment_evidence_preserves_order_headers_and_residues() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let registry = alignment_registry(root.path());
    let source = root.path().join("input.afa");
    let long = "aCgT-".repeat(13);
    let original = format!(
        ">second description with spaces\r\n{}\r\n{}\r\n>first another description\r\n{}\r\n",
        &long[..17],
        &long[17..],
        "aC--.".repeat(13)
    );
    std::fs::write(&source, &original).unwrap();
    let mut manager = JobManager::default();
    let accepted = manager
        .submit(
            &registry,
            serde_json::from_value(json!({
                "operation":"org.example.registry-fixture/count@1",
                "inputs":{"sources":[{"kind":"file","path":"input.afa"}]},
                "parameters":{"mode":"normal"},"destinations":{"report":"result.json"}
            }))
            .unwrap(),
            SessionPolicy::new(root.path(), true).unwrap(),
            "test",
        )
        .unwrap();
    let outcome = wait(&manager, &accepted.run_id);
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:#?}");
    let run = Path::new(outcome.evidence.as_ref().unwrap());
    let record = basecraft_ops::records::read_input(run).unwrap();
    assert_eq!(record.transformation, "canonical-fasta-v1");
    assert_eq!(
        record.source.unwrap().hash,
        basecraft_io::hash_bytes(original.as_bytes())
    );
    let second = "aC--.".repeat(13);
    let canonical = format!(
        ">second description with spaces\n{}\n{}\n>first another description\n{}\n{}\n",
        &long[..60],
        &long[60..],
        &second[..60],
        &second[60..]
    );
    assert_eq!(
        record.submitted_hash,
        basecraft_io::hash_bytes(canonical.as_bytes())
    );
    assert_ne!(
        record.submitted_hash,
        basecraft_io::hash_bytes(original.as_bytes())
    );
    assert_eq!(outcome.data, Some(json!({"bytes":canonical.len()})));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
    let publication =
        basecraft_ops::records::inspect(&root.path().join("result.json"), None).unwrap();
    assert!(
        publication
            .events
            .iter()
            .any(|event| event.op.is_run(&outcome.run_id))
    );
}

#[test]
fn unequal_alignment_inputs_are_refused_before_execution_for_files_and_documents() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let registry = alignment_registry(root.path());
    let source = root.path().join("input.afa");
    let original = ">a description\nAC-GT\n>b trailing gap\nAC-\n";
    std::fs::write(&source, original).unwrap();
    let mut session = basecraft_ops::Session::new(root.path());
    let doc = session.open(&source).unwrap();
    // Loading still supports the rectangular editor, but cannot erase evidence.
    assert_eq!(session.alignment(doc).unwrap().read(1, 0, 5), b"AC---");
    assert!(
        session
            .alignment(doc)
            .unwrap()
            .snapshot()
            .validate_aligned_input()
            .is_err()
    );
    let mut manager = JobManager::default();
    let expected = "plugin protocol: This operation requires aligned sequences of equal length. These sequences have different lengths; align them first.";
    for route in ["file", "clean", "edited", "undo"] {
        if route == "edited" {
            session
                .apply(
                    doc,
                    basecraft_protocol::Op::AlnReplace {
                        row: 0,
                        start: 0,
                        end: 1,
                        residues: "T".into(),
                    },
                )
                .unwrap();
        } else if route == "undo" {
            session.undo(doc).unwrap();
        }
        let input = if route == "file" {
            json!({"kind":"file","path":"input.afa"})
        } else {
            json!({"kind":"document","doc":doc.0.to_string(),
                "generation":session.generation(doc).unwrap(),"revision":session.revision(doc).unwrap()})
        };
        let request = serde_json::from_value(json!({
            "operation":"org.example.registry-fixture/count@1", "inputs":{"sources":[input]},
            "parameters":{"mode":"normal"},"destinations":{"report":"result.json"}
        }))
        .unwrap();
        let result = manager.submit_in_session(
            &registry,
            request,
            SessionPolicy::new(root.path(), true).unwrap(),
            "test",
            &mut session,
        );
        assert_eq!(result.unwrap_err().to_string(), expected, "{route}");
        assert!(!root.path().join("result.json").exists());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
    }
}

#[test]
fn ungapped_alignment_input_accepts_unequal_lengths_without_padding() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let registry = fasta_registry(root.path(), "ungapped_fasta");
    let source = root.path().join("input.afa");
    let original = ">a description\nAC-GT\n>b trailing gap\nAC-\n";
    std::fs::write(&source, original).unwrap();
    let mut session = basecraft_ops::Session::new(root.path());
    let doc = session.open(&source).unwrap();
    let mut manager = JobManager::default();
    for route in ["file", "clean", "edited"] {
        if route == "edited" {
            session
                .apply(
                    doc,
                    basecraft_protocol::Op::AlnReplace {
                        row: 0,
                        start: 0,
                        end: 1,
                        residues: "T".into(),
                    },
                )
                .unwrap();
        }
        let input = if route == "file" {
            json!({"kind":"file","path":"input.afa"})
        } else {
            json!({"kind":"document","doc":doc.0.to_string(),
                "generation":session.generation(doc).unwrap(),"revision":session.revision(doc).unwrap()})
        };
        let outcome = run_in_session(
            &mut manager,
            &registry,
            &mut session,
            json!({
                "operation":"org.example.registry-fixture/count@1", "inputs":{"sources":[input]},
                "parameters":{"mode":"normal"},"destinations":{"report":format!("{route}.json")}
            }),
        );
        assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
        let record =
            basecraft_ops::records::read_input(Path::new(outcome.evidence.as_ref().unwrap()))
                .unwrap();
        let first = if route == "edited" { "TCGT" } else { "ACGT" };
        let submitted = format!(">a\n{first}\n>b\nAC\n");
        assert_eq!(
            record.submitted_hash,
            basecraft_io::hash_bytes(submitted.as_bytes())
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
    }
}

#[test]
fn cancelled_document_run_leaves_no_success_history() {
    use basecraft_protocol::records::JobStatus;
    crate::isolated_profile::isolate!();
    for mode in ["slow", "hang"] {
        let temp = tempfile::tempdir().unwrap();
        let registry = alignment_registry(temp.path());
        let source = temp.path().join("input.afa");
        std::fs::write(&source, ">a\nACGT\n>b\nACGT\n").unwrap();
        let mut session = basecraft_ops::Session::new(temp.path());
        let doc = session.open(&source).unwrap();
        let mut manager = JobManager::default();
        let request = serde_json::from_value(json!({
            "operation":"org.example.registry-fixture/count@1",
            "inputs":{"sources":[{"kind":"document","doc":doc.0.to_string(),
                "generation":session.generation(doc).unwrap(),"revision":session.revision(doc).unwrap()}]},
            "parameters":{"mode":mode},"destinations":{"report":"cancelled.json"}
        }))
        .unwrap();
        let accepted = manager
            .submit_in_session(
                &registry,
                request,
                SessionPolicy::new(temp.path(), true).unwrap(),
                "GUI",
                &mut session,
            )
            .unwrap();
        let id = &accepted.run_id;
        let deadline = Instant::now() + Duration::from_secs(30);
        while manager.get(id).unwrap().state == RuntimeState::Queued {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(250));
        manager.cancel(id).unwrap();
        let outcome = wait(&manager, id);
        assert_eq!(outcome.state, RuntimeState::Cancelled, "{outcome:?}");
        assert_eq!(outcome.artifacts, Vec::new());
        assert!(!temp.path().join("cancelled.json").exists());
        manager.poll_session(&mut session);
        let record = session.document_record(doc).unwrap();
        assert!(
            !record
                .events
                .iter()
                .any(|event| matches!(&event.op, basecraft_protocol::Op::ToolRun { .. }))
        );
        let finished: Vec<_> = record
            .events
            .iter()
            .filter_map(|event| match &event.op {
                basecraft_protocol::Op::JobFinished { report } if report.id == *id => Some(report),
                _ => None,
            })
            .collect();
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].status, JobStatus::Cancelled);
        assert_eq!(finished[0].outputs, std::collections::BTreeMap::new());
    }
}

#[test]
fn retained_inputs_survive_job_drop_and_temporary_source_removal() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("a.txt"), b"abc\n").unwrap();
    std::fs::write(temp.path().join("b.txt"), b"xy\n").unwrap();
    let registry = registry_retention(temp.path(), true);
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(
            &registry,
            request("normal"),
            SessionPolicy::new(temp.path(), true).unwrap(),
            "retained input test",
        )
        .unwrap();
    let outcome = wait(&jobs, &accepted.run_id);
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
    let execution: serde_json::Value = serde_json::from_slice(
        &std::fs::read(Path::new(outcome.evidence.as_ref().unwrap()).join("execution.json"))
            .unwrap(),
    )
    .unwrap();
    let run = Path::new(outcome.evidence.as_ref().unwrap());
    let inputs: Vec<_> = (0..execution["inputs"]["sources"].as_array().unwrap().len())
        .map(|index| run.join(format!("submitted-sources-{index}")))
        .collect();
    drop(jobs);
    std::fs::remove_file(temp.path().join("a.txt")).unwrap();
    std::fs::remove_file(temp.path().join("b.txt")).unwrap();
    assert_eq!(std::fs::read(&inputs[0]).unwrap(), b"abc\n");
    assert_eq!(std::fs::read(&inputs[1]).unwrap(), b"xy\n");
    let issues = basecraft_ops::records::verify(&temp.path().join("report.json"), None).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    std::fs::write(&inputs[0], b"damaged").unwrap();
    assert_ne!(
        basecraft_ops::records::verify(&temp.path().join("report.json"), None).unwrap(),
        Vec::<String>::new()
    );
}

/// Exercise the real registered runner, with equal original bytes but different
/// paths, ledgers and submitted transformations. Path ordering must not own data.
#[test]
fn equal_content_retention_keeps_exact_primary_owner_and_leaf_lifecycles() {
    use basecraft_ops::records::{self, AffectedAnalysis, EvidenceConflict, SavePolicy};
    crate::isolated_profile::isolate!();
    for retain_primary in [false, true] {
        for reverse_paths in [false, true] {
            for policy in [SavePolicy::BreakEvidence, SavePolicy::RemoveAnalysis] {
                let temp = tempfile::tempdir().unwrap();
                let (first, other) = if reverse_paths {
                    ("z.afa", "a.afa")
                } else {
                    ("a.afa", "z.afa")
                };
                let original = b">row description\nAC-G\n";
                std::fs::write(temp.path().join(first), original).unwrap();
                std::fs::write(temp.path().join(other), original).unwrap();
                let registry = registry_inputs(
                    temp.path(),
                    false,
                    Some(json!({
                        "a_primary": {"view":"ungapped_fasta", "media_type":"text/x-fasta",
                            "min":1, "max":1, "retain":retain_primary},
                        "sources": {"view":"bytes", "media_type":"application/octet-stream",
                            "min":1, "max":1, "retain":true}
                    })),
                );
                let request = serde_json::from_value(json!({
                    "operation":"org.example.registry-fixture/count@1",
                    "inputs":{"a_primary":[{"kind":"file","path":first}],
                        "sources":[{"kind":"file","path":other}]},
                    "parameters":{"mode":"normal"}, "destinations":{"report":"report.json"}
                }))
                .unwrap();
                let mut jobs = JobManager::default();
                let accepted = jobs
                    .submit(
                        &registry,
                        request,
                        SessionPolicy::new(temp.path(), true).unwrap(),
                        "equal originals",
                    )
                    .unwrap();
                let outcome = wait(&jobs, &accepted.run_id);
                assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
                let run = Path::new(outcome.evidence.as_ref().unwrap());
                let execution: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(run.join("execution.json")).unwrap())
                        .unwrap();
                let evidence = &execution["inputs"];
                let a = &evidence["a_primary"][0];
                let b = &evidence["sources"][0];
                let owner = &run.join(a["run_directory"].as_str().unwrap());
                let sibling = &run.join(b["run_directory"].as_str().unwrap());
                assert_ne!(owner, sibling);
                assert_ne!(a["original_path"], b["original_path"]);
                assert_eq!(a["source_hash"], b["source_hash"]);
                assert_ne!(a["submitted_hash"], a["source_hash"]);
                assert_eq!(b["submitted_hash"], b["source_hash"]);
                let primary = records::read_input(run).unwrap();
                let source = primary.source.as_ref().unwrap();
                assert_eq!(
                    run.join(&source.path).canonicalize().unwrap(),
                    temp.path().join(first).canonicalize().unwrap()
                );
                // This ownership assertion fails on the former hash-only lookup
                // when only the equal-byte sibling binding is retained.
                if retain_primary {
                    let origin = primary.origin_run.as_deref().unwrap();
                    assert_eq!(
                        run.join(origin).canonicalize().unwrap(),
                        owner.canonicalize().unwrap()
                    );
                    assert_eq!(
                        primary.state,
                        basecraft_protocol::input_records::EvidenceState::Preserved
                    );
                    assert_eq!(
                        std::fs::read(owner.join("preserved-input")).unwrap(),
                        original
                    );
                } else {
                    assert!(
                        primary.origin_run.is_none(),
                        "unretained primary delegated to a distinct equal-byte original"
                    );
                    assert_eq!(
                        primary.state,
                        basecraft_protocol::input_records::EvidenceState::Referenced
                    );
                    assert_eq!(
                        records::read_input(owner).unwrap().state,
                        basecraft_protocol::input_records::EvidenceState::Referenced
                    );
                }
                assert_eq!(
                    std::fs::read(sibling.join("preserved-input")).unwrap(),
                    original
                );
                if retain_primary {
                    let basecraft_ops::files::Loaded::Alignment(loaded) =
                        basecraft_ops::files::load(&temp.path().join(first)).unwrap()
                    else {
                        panic!("fixture alignment")
                    };
                    let mut expected = Vec::new();
                    basecraft_ops::files::write_ungapped(&loaded.doc, &mut expected).unwrap();
                    let submitted = run.join("submitted-a_primary-0");
                    assert_eq!(std::fs::read(submitted).unwrap(), expected);
                }
                assert_eq!(
                    std::fs::read(run.join("submitted-sources-0")).unwrap(),
                    original
                );
                assert_eq!(
                    basecraft_ops::records::verify(&temp.path().join("report.json"), None).unwrap(),
                    Vec::<String>::new()
                );
                drop(jobs);
                // Direct leaf lifecycle decisions must not rewrite a distinct
                // primary/owner. Full logical analysis closure still includes B.
                let before = std::fs::read(run.join("input.json")).unwrap();
                let owner_before = std::fs::read(owner.join("input.json")).unwrap();
                let conflict = EvidenceConflict {
                    target: temp.path().join(other),
                    analyses: vec![AffectedAnalysis {
                        run: sibling.into(),
                        result: "other leaf".into(),
                        source_hash: b["source_hash"].as_str().unwrap().into(),
                        can_preserve: true,
                        direct: true,
                        backing_analyses: vec![],
                    }],
                    options: vec![policy],
                };
                {
                    let _mutation = records::Mutation::begin().unwrap();
                    records::resolve(&conflict, policy).unwrap();
                }
                assert_eq!(std::fs::read(run.join("input.json")).unwrap(), before);
                assert_eq!(
                    std::fs::read(owner.join("input.json")).unwrap(),
                    owner_before
                );
                if policy == SavePolicy::BreakEvidence {
                    assert_eq!(
                        records::read_input(sibling).unwrap().state,
                        basecraft_protocol::input_records::EvidenceState::Broken
                    );
                } else {
                    assert!(!sibling.exists());
                }
                let leaf = records::read_input(owner).unwrap();
                let live = records::verified_source(owner, leaf.source.as_ref().unwrap()).unwrap();
                assert_eq!(std::fs::read(live).unwrap(), original);
                if retain_primary {
                    std::fs::remove_file(temp.path().join(first)).unwrap();
                    let retained =
                        records::verified_source(owner, leaf.source.as_ref().unwrap()).unwrap();
                    assert_eq!(std::fs::read(retained).unwrap(), original);
                }
                assert_eq!(std::fs::read(temp.path().join(other)).unwrap(), original);
            }
        }
    }
}

#[test]
fn registered_target_preparation_preserves_permissions_and_session_identity() {
    use basecraft_plugin_host::runner::PreparedOperation;
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), b"a").unwrap();
    std::fs::write(root.path().join("b.txt"), b"b").unwrap();
    std::fs::write(root.path().join("target.afa"), ">a\nACGT\n>b\nAC-T\n").unwrap();
    let registry = registry(root.path());
    let operation = registry
        .resolve(&request("normal").operation, None)
        .unwrap();
    let prepare =
        |target: Option<serde_json::Value>, write, session: Option<&mut basecraft_ops::Session>| {
            let mut request = request("normal");
            request.target = target.map(|value| serde_json::from_value(value).unwrap());
            let mut policy = SessionPolicy::new(root.path(), true).unwrap();
            policy.document_write = write;
            PreparedOperation::prepare(
                operation.clone(),
                request,
                policy,
                "test".into(),
                session,
                &registry,
                &[],
            )
        };
    // No explicit target needs no session, even without document-write permission.
    assert!(prepare(None, false, None).is_ok());
    let file = json!({"kind":"file","path":"target.afa"});
    assert!(
        prepare(Some(file.clone()), false, None)
            .err()
            .unwrap()
            .to_string()
            .contains("a target document needs an owning session")
    );
    let mut session = basecraft_ops::Session::new(root.path());
    // A missing/out-of-root path must not obscure the permission error or open a document.
    for path in ["target.afa", "missing.afa", "../outside.afa"] {
        let error = prepare(
            Some(json!({"kind":"file","path":path})),
            false,
            Some(&mut session),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(
            error.contains("permission_denied: saving a target document needs writes"),
            "{error}"
        );
        assert!(!session.is_open(basecraft_ops::DocId(0)));
    }
    let prepared = prepare(Some(file), true, Some(&mut session)).unwrap();
    let target = prepared.document.as_ref().unwrap();
    assert!(target.save);
    let id = target.doc;
    assert!(session.is_open(id));
    let document = json!({"kind":"document","doc":id.0.to_string(),
        "generation":session.generation(id).unwrap(),"revision":session.revision(id).unwrap()});
    let prepared = prepare(Some(document.clone()), false, Some(&mut session)).unwrap();
    let target = prepared.document.as_ref().unwrap();
    assert_eq!(target.doc, id);
    assert!(!target.save);
    for (field, value, message) in [
        ("doc", json!("bad-id"), "invalid session document ID"),
        (
            "doc",
            json!(usize::MAX.to_string()),
            "stale_revision: target document changed",
        ),
        (
            "generation",
            json!(session.generation(id).unwrap() + 1),
            "stale_revision: target document changed",
        ),
        (
            "revision",
            json!(session.revision(id).unwrap() + 1),
            "stale_revision: target document changed",
        ),
    ] {
        let mut stale = document.clone();
        stale[field] = value;
        let error = prepare(Some(stale), true, Some(&mut session))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
#[cfg(feature = "test-counters")]
fn captured_inputs_reach_the_registered_caller_without_rereading_originals() {
    use basecraft_ops::sanger_review::CapturedInput;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let paths = [root.join("a.txt"), root.join("b.txt")];
    std::fs::write(&paths[0], b"abc\n").unwrap();
    std::fs::write(&paths[1], b"xy\n").unwrap();
    let registry = registry(root);
    basecraft_io::counters::reset();
    let captures = paths
        .iter()
        .map(|path| CapturedInput::open(path).unwrap())
        .collect();
    let mut jobs = JobManager::default().with_captured_inputs(captures);
    let accepted = jobs
        .submit(
            &registry,
            request("normal"),
            SessionPolicy::new(root, true).unwrap(),
            "captured inputs",
        )
        .unwrap();
    let result = wait(&jobs, &accepted.run_id);
    assert_eq!(result.state, RuntimeState::Completed, "{result:?}");
    assert_eq!(result.data.as_ref().unwrap()["bytes"], 7);
    let counts = basecraft_io::counters::snapshot();
    for path in paths {
        assert_eq!(
            counts.reads[&path.canonicalize().unwrap()].full_reads,
            1,
            "{counts:?}"
        );
    }
}
