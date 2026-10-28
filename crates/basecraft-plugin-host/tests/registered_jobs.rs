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
    assert_eq!(
        basecraft_ops::provenance::verify(&output).unwrap(),
        Vec::<String>::new()
    );
    std::fs::write(temp.path().join("b.txt"), b"changed").unwrap();
    assert_ne!(
        basecraft_ops::provenance::verify(&output).unwrap(),
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
        basecraft_ops::provenance::verify(&temp.path().join("report.json")).unwrap(),
        Vec::<String>::new()
    );
    for name in ["a.txt", "b.txt"] {
        assert_eq!(std::fs::read(temp.path().join(name)).unwrap(), original);
    }
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
    assert_eq!(session.doc(doc).feature_count(), 1);
    assert_eq!(session.doc(doc).read(0, 8), b"TCGTACGT");
    assert_eq!(std::fs::read(&source).unwrap(), b">sequence\nACGTACGT\n");
}

#[test]
fn hostile_processes_and_unallocated_outputs_never_publish_success() {
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
        basecraft_ops::provenance::verify(&temp.path().join("report.json")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn late_collision_preserves_foreign_bytes_and_repairs_without_reexecuting() {
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
                "generation":session.generation(doc),"revision":session.doc(doc).version()}]},
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
        assert_eq!(session.doc(doc).feature_count(), 1);
        let activities = || {
            session
                .document_record(doc)
                .unwrap()
                .events
                .into_iter()
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
            session.doc(doc).feature_count(),
            1,
            "repair must not reapply edits"
        );
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).feature_count(), 0);
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
    let temp = tempfile::tempdir().unwrap();
    let registry = alignment_registry(temp.path());
    let source = temp.path().join("input.afa");
    std::fs::write(&source, ">a\nAC-T\n>b\nACGT\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&source).unwrap();
    let mut manager = JobManager::default();
    let document = |session: &Session| json!([{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc),"revision":session.revision(doc)}]);
    // A clean document is submitted as its saved file.
    let request = json!({"operation":"org.example.registry-fixture/count@1","inputs":{"sources":document(&session)},
            "parameters":{"mode":"normal"},"destinations":{"report":"clean.json"}});
    let clean = run_in_session(&mut manager, &registry, &mut session, request);
    assert_eq!(clean.state, RuntimeState::Completed, "{clean:?}");
    assert_eq!(
        basecraft_ops::provenance::verify(&temp.path().join("clean.json")).unwrap(),
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
    let publication = basecraft_ops::provenance::inspect(&temp.path().join("edited.json")).unwrap();
    assert!(publication.events.iter().any(|event| matches!(&event.op,
        basecraft_protocol::Op::ToolRun { params, .. } if params["run_id"] == edited.run_id)));
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
            basecraft_protocol::Op::ToolRun { params, .. } => params["run_id"].as_str(),
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
                basecraft_protocol::delimitation::JobStatus::Completed
            ),
            (
                edited.run_id.as_str(),
                basecraft_protocol::delimitation::JobStatus::Completed
            )
        ]
    );
    // The Tasks view lists registered runs and points at their own record.
    let tasks = basecraft_ops::document_record::jobs(true).unwrap();
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
                && report.status == basecraft_protocol::delimitation::JobStatus::Failed)));
    // A request pinned to an older revision is refused before anything runs.
    let stale = manager.submit_in_session(
        &registry,
        serde_json::from_value(json!({"operation":"org.example.registry-fixture/count@1",
            "inputs":{"sources":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc),"revision":0}]},
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
fn document_repair_waits_for_owner_history_and_does_not_repeat_activity() {
    use basecraft_ops::Session;
    let root = tempfile::tempdir().unwrap();
    let registry = alignment_registry(root.path());
    let source = root.path().join("input.afa");
    std::fs::write(&source, ">a\nACGT\n>b\nAC-T\n").unwrap();
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    let mut manager = JobManager::default();
    let accepted = manager.submit_in_session(&registry, serde_json::from_value(json!({
        "operation":"org.example.registry-fixture/count@1",
        "inputs":{"sources":[{"kind":"document","doc":doc.0.to_string(),"generation":session.generation(doc),"revision":session.revision(doc)}]},
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
            .filter(|event| matches!(&event.op,
        basecraft_protocol::Op::ToolRun { params, .. } if params["run_id"] == accepted.run_id))
            .count(),
        1
    );
    assert_eq!(
        record
            .events
            .iter()
            .filter(|event| matches!(&event.op,
        basecraft_protocol::Op::JobFinished { report } if report.id == accepted.run_id
            && report.status == basecraft_protocol::delimitation::JobStatus::Completed))
            .count(),
        1
    );
    let publication = basecraft_ops::provenance::inspect(&output).unwrap();
    assert!(publication.events.iter().any(|event| matches!(&event.op,
        basecraft_protocol::Op::ToolRun { params, .. } if params["run_id"] == accepted.run_id)));
}

#[test]
fn canonical_alignment_evidence_preserves_order_headers_and_residues() {
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
    let record = basecraft_ops::input_evidence::read_input(run).unwrap();
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
    let publication = basecraft_ops::provenance::inspect(&root.path().join("result.json")).unwrap();
    assert!(publication.events.iter().any(|event| matches!(&event.op,
        basecraft_protocol::Op::ToolRun { params, .. } if params["run_id"] == outcome.run_id)));
}

#[test]
fn unequal_alignment_inputs_are_refused_before_execution_for_files_and_documents() {
    let root = tempfile::tempdir().unwrap();
    let registry = alignment_registry(root.path());
    let source = root.path().join("input.afa");
    let original = ">a description\nAC-GT\n>b trailing gap\nAC-\n";
    std::fs::write(&source, original).unwrap();
    let mut session = basecraft_ops::Session::new(root.path());
    let doc = session.open(&source).unwrap();
    // Loading still supports the rectangular editor, but cannot erase evidence.
    assert_eq!(session.alignment(doc).read(1, 0, 5), b"AC---");
    assert!(
        session
            .alignment(doc)
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
                "generation":session.generation(doc),"revision":session.revision(doc)})
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
                "generation":session.generation(doc),"revision":session.revision(doc)})
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
        let record = basecraft_ops::input_evidence::read_input(Path::new(
            outcome.evidence.as_ref().unwrap(),
        ))
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
fn tasks_cancel_registered_document_run_through_owner_without_success_history() {
    use basecraft_plugin_host::file_tool::{can_cancel_run, cancel_run};
    use basecraft_protocol::delimitation::JobStatus;
    for mode in ["slow", "hang"] {
        let temp = tempfile::tempdir().unwrap();
        let registry = alignment_registry(temp.path());
        let source = temp.path().join("input.afa");
        std::fs::write(&source, ">a\nACGT\n>b\nACGT\n").unwrap();
        let mut session = basecraft_ops::Session::new(temp.path());
        let doc = session.open(&source).unwrap();
        let mut manager = JobManager::default().with_task_cancellation();
        let request = serde_json::from_value(json!({
            "operation":"org.example.registry-fixture/count@1",
            "inputs":{"sources":[{"kind":"document","doc":doc.0.to_string(),
                "generation":session.generation(doc),"revision":session.revision(doc)}]},
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
        assert!(can_cancel_run(id));
        assert!(!can_cancel_run("unknown-run"));
        assert!(cancel_run("unknown-run").is_err());
        cancel_run(id).unwrap();
        assert!(!can_cancel_run(id));
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
        assert!(!can_cancel_run(id));
        assert!(cancel_run(id).is_err());
        drop(manager);
        assert!(!can_cancel_run(id));
        assert!(cancel_run(id).is_err());
    }
}

#[test]
fn tasks_refuse_headless_completed_and_dropped_owners() {
    use basecraft_plugin_host::file_tool::{can_cancel_run, cancel_run};
    let temp = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(temp.path().join(name), "a").unwrap();
    }
    let registry = registry(temp.path());
    let policy = || SessionPolicy::new(temp.path(), true).unwrap();
    let mut headless = JobManager::default();
    let private = headless
        .submit(&registry, request("hang"), policy(), "CLI")
        .unwrap();
    assert!(!can_cancel_run(&private.run_id));
    assert!(cancel_run(&private.run_id).is_err());
    headless.cancel(&private.run_id).unwrap();
    assert_eq!(
        wait(&headless, &private.run_id).state,
        RuntimeState::Cancelled
    );
    let mut gui = JobManager::default().with_task_cancellation();
    let completed = gui
        .submit(&registry, request("normal"), policy(), "GUI")
        .unwrap();
    assert_eq!(wait(&gui, &completed.run_id).state, RuntimeState::Completed);
    assert!(!can_cancel_run(&completed.run_id));
    assert!(cancel_run(&completed.run_id).is_err());
    let mut pending = request("hang");
    pending
        .destinations
        .insert("report".into(), "pending.json".into());
    let active = gui.submit(&registry, pending, policy(), "GUI").unwrap();
    assert!(can_cancel_run(&active.run_id));
    drop(gui);
    assert!(!can_cancel_run(&active.run_id));
    assert!(cancel_run(&active.run_id).is_err());
}

#[test]
fn tasks_refuse_document_publication_awaiting_owner_finalization() {
    use basecraft_plugin_host::file_tool::{can_cancel_run, cancel_run};
    let temp = tempfile::tempdir().unwrap();
    let registry = alignment_registry(temp.path());
    let source = temp.path().join("input.afa");
    std::fs::write(&source, ">a\nACGT\n>b\nACGT\n").unwrap();
    let mut session = basecraft_ops::Session::new(temp.path());
    let doc = session.open(&source).unwrap();
    let mut manager = JobManager::default().with_task_cancellation();
    let request = serde_json::from_value(json!({
        "operation":"org.example.registry-fixture/count@1",
        "inputs":{"sources":[{"kind":"document","doc":doc.0.to_string(),
            "generation":session.generation(doc),"revision":session.revision(doc)}]},
        "parameters":{},"destinations":{"report":"report.json"}
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
    while manager.get(id).unwrap().state != RuntimeState::Finalizing {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!can_cancel_run(id));
    assert!(cancel_run(id).is_err());
    assert_eq!(manager.get(id).unwrap().state, RuntimeState::Finalizing);
    loop {
        manager.poll_session(&mut session);
        let outcome = manager.get(id).unwrap();
        if outcome.state.terminal() {
            assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(temp.path().join("report.json").exists());
    assert!(!can_cancel_run(id));
    assert!(cancel_run(id).is_err());
}
