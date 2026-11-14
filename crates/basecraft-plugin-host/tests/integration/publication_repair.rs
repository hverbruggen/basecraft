//! Faults after visible publication must resume owned work without rerunning tools.
use crate::common;

use basecraft_ops::{Session, records};
use basecraft_plugin_host::{jobs::JobManager, policy::SessionPolicy};
use basecraft_protocol::operation::RuntimeState;
use serde_json::json;

const ALIGNMENT: &str = ">a first\nACGT\n>b second\nAC-T\n>c third\nTCGT\n";
const TREE_FIXTURE: &str = r#"
import time
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin("basecraft.fasttree", "test")
@plugin.operation("tree")
def tree(_parameters, context):
    time.sleep(0.3)
    root = Path(context.outputs["artifacts"])
    (root / "tree.nwk").write_text("((a:0.1,b:0.2)0.9:0.1,c:0.3);\n")
    return {"artifacts":[{"role":"artifacts","path":"tree.nwk"}]}
plugin.run()
"#;

#[test]
fn member_metadata_repair_requires_owned_bytes_and_retries_companions() {
    crate::isolated_profile::isolate!();
    for replace_with_foreign in [false, true] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("input.afa"), ALIGNMENT).unwrap();
        let bundled = common::fixture_package(root.path(), "fasttree", "tree", TREE_FIXTURE);
        let registry = common::registry(&bundled);
        let mut jobs = JobManager::default();
        let accepted = jobs.submit(&registry, serde_json::from_value(json!({
            "operation":"basecraft.fasttree/tree@1","inputs":{"alignment":common::file("input.afa")},
            "parameters":{},"destinations":{"artifacts/tree.nwk":"result.nwk"}
        })).unwrap(), SessionPolicy::new(root.path(), true).unwrap(), "test").unwrap();
        let run = common::evidence(&accepted);
        let fault = run.join("result.nwk.basecraft.json");
        std::fs::create_dir(&fault).unwrap();
        let partial = common::wait(&jobs, &accepted.run_id);
        assert_eq!(partial.state, RuntimeState::Partial, "{partial:#?}");
        let output = root.path().join("result.nwk");
        assert!(output.is_file(), "fault occurs after visible export");
        let bytes = std::fs::read(&output).unwrap();
        let execution = std::fs::read(run.join("execution.json")).unwrap();
        std::fs::remove_dir(&fault).unwrap();
        if replace_with_foreign {
            let foreign = root.path().join("foreign.nwk");
            std::fs::write(&foreign, &bytes).unwrap();
            std::fs::rename(foreign, &output).unwrap();
        }
        let repaired = jobs.repair(&accepted.run_id).unwrap();
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
        assert_eq!(
            std::fs::read(run.join("execution.json")).unwrap(),
            execution
        );
        if replace_with_foreign {
            assert_eq!(repaired.state, RuntimeState::Partial, "{repaired:#?}");
            assert!(!fault.exists());
        } else {
            common::assert_completed(&repaired);
            assert!(fault.is_file());
            assert!(basecraft_ops::records::sidecar(&output).is_file());
            assert_eq!(
                records::verify(&output, None).unwrap(),
                Vec::<String>::new()
            );
        }
    }
}

#[test]
fn workspace_inventory_survives_repair_and_rejects_changed_export() {
    crate::isolated_profile::isolate!();
    for corrupt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("tree.nwk"), "(a:0.1,b:0.2);\n").unwrap();
        let registry = common::bundled();
        let mut jobs = JobManager::default();
        let mut session = Session::new(root.path());
        let doc = session.open(&root.path().join("tree.nwk")).unwrap();
        let accepted = common::submit_in(&mut jobs, &registry, &mut session, |s| {
            json!({
                "operation":"basecraft.delimitation/workspace_tree@1",
                "inputs":{"tree":common::document(s, doc)},"parameters":{},
                "destinations":{"workspace":"species.spart"}
            })
        })
        .unwrap();
        let run = common::evidence(&accepted);
        let fault = run.join("manifest.json");
        std::fs::create_dir(&fault).unwrap();
        let partial = common::wait(&jobs, &accepted.run_id);
        assert_eq!(partial.state, RuntimeState::Partial, "{partial:#?}");
        let output = root.path().join("species.spart");
        assert!(output.is_file());
        assert!(
            partial
                .artifacts
                .iter()
                .any(|artifact| artifact.path == output.display().to_string())
        );
        std::fs::remove_dir(&fault).unwrap();
        if corrupt {
            std::fs::write(&output, b"changed").unwrap();
        }
        let repaired = jobs.repair(&accepted.run_id).unwrap();
        if corrupt {
            assert_eq!(repaired.state, RuntimeState::Partial);
            assert_eq!(std::fs::read(&output).unwrap(), b"changed");
        } else {
            assert_eq!(repaired.state, RuntimeState::Finalizing);
            let completed = common::wait_in(&jobs, &mut session, &accepted.run_id);
            common::assert_completed(&completed);
            let publication = records::inspect(&output, None).unwrap();
            assert!(
                publication
                    .outputs
                    .iter()
                    .any(|artifact| artifact.logical_name == "species.spart")
            );
            assert_eq!(
                records::verify(&output, None).unwrap(),
                Vec::<String>::new()
            );
        }
    }
}

#[test]
fn owner_save_failure_stays_partial_and_repair_does_not_reapply_hypotheses() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree.nwk");
    std::fs::write(&tree, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let registry = common::bundled();
    let mut session = Session::new(root.path());
    let input = session.open(&tree).unwrap();
    let setup = common::run_in(&registry, &mut session, |s| {
        json!({
            "operation":"basecraft.delimitation/workspace_tree@1",
            "inputs":{"tree":common::document(s, input)},"parameters":{},
            "destinations":{"workspace":"species.spart"}
        })
    });
    common::assert_completed(&setup);
    let workspace = root.path().join("species.spart");
    let mut jobs = JobManager::default();
    let accepted = common::submit_in(&mut jobs, &registry, &mut session, |_| json!({
        "operation":"basecraft.delimitation/manual_cut@1",
        "inputs":{"tree":common::file("tree.nwk")},"parameters":{"rooted":true,"threshold":0.25},
        "target":{"kind":"file","path":"species.spart"}
    })).unwrap();
    let target = jobs.target_document(&accepted.run_id).unwrap();
    // Wait only for worker publication; owner-thread application is still pending.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !common::evidence(&accepted).join("manifest.json").is_file() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Fail the save intent after typed application; earlier journal writes work.
    let before = std::fs::read(&workspace).unwrap();
    let fault = session.recovery_root().join("pending-save.json");
    std::fs::create_dir(&fault).unwrap();
    let partial = common::wait_in(&jobs, &mut session, &accepted.run_id);
    assert_eq!(partial.state, RuntimeState::Partial, "{partial:#?}");
    let count = session.delimitation(target).unwrap().hypotheses.len();
    assert!(count > 0, "save failed after hypotheses were committed");
    assert_eq!(std::fs::read(&workspace).unwrap(), before);
    std::fs::remove_dir(&fault).unwrap();
    assert_eq!(
        jobs.repair(&accepted.run_id).unwrap().state,
        RuntimeState::Finalizing
    );
    let completed = common::wait_in(&jobs, &mut session, &accepted.run_id);
    common::assert_completed(&completed);
    assert_eq!(
        session.delimitation(target).unwrap().hypotheses.len(),
        count
    );
    assert_eq!(
        basecraft_ops::files::load_delimitation(&workspace)
            .unwrap()
            .hypotheses
            .len(),
        count
    );
    assert_eq!(common::tool_run_events(&session, target).len(), 1);
    assert!(jobs.finalized_generation(&accepted.run_id).is_some());
    drop(jobs);
    drop(session);
    assert_reopened_completion(&workspace, &accepted.run_id, count);
}

/// Saved and portable history must stand on their own without the owner journal.
fn assert_reopened_completion(workspace: &std::path::Path, run_id: &str, hypotheses: usize) {
    let mut reopened = Session::recoverable(workspace.parent().unwrap()).unwrap();
    let doc = reopened.open(workspace).unwrap();
    assert_eq!(
        reopened.delimitation(doc).unwrap().hypotheses.len(),
        hypotheses
    );
    let record = reopened.document_record(doc).unwrap();
    let finished: Vec<_> = record
        .events
        .iter()
        .filter(|event| {
            matches!(&event.op,
        basecraft_protocol::Op::JobFinished { report } if report.id == run_id
            && report.status == basecraft_protocol::records::JobStatus::Completed)
        })
        .collect();
    assert_eq!(
        finished.len(),
        1,
        "saved workspace lost or duplicated terminal history: {run_id}"
    );
    let report = record.report().unwrap();
    assert!(report.history.iter().any(|row| row.id == finished[0].id));
    assert_eq!(
        record
            .events
            .iter()
            .filter(|event| event.op.is_run(run_id))
            .count(),
        1
    );
    let portable = workspace.parent().unwrap().join("portable-completion");
    records::package(workspace, &portable, None).unwrap();
    let portable_result = portable.join(workspace.file_name().unwrap());
    assert_eq!(
        records::verify(&portable_result, None).unwrap(),
        Vec::<String>::new()
    );
    let publication = records::inspect(&portable_result, None).unwrap();
    assert_eq!(
        publication
            .events
            .iter()
            .filter(|event| matches!(&event.op,
        basecraft_protocol::Op::JobFinished { report } if report.id == run_id
            && report.status == basecraft_protocol::records::JobStatus::Completed))
            .count(),
        1
    );
    let portable_record =
        basecraft_ops::records::DocumentRecord::from_file(&portable_result, None).unwrap();
    assert!(
        portable_record
            .report()
            .unwrap()
            .history
            .iter()
            .any(|row| row.id == finished[0].id)
    );
}

#[test]
fn automatically_saved_workspace_retains_completion_after_owner_closes() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree.nwk");
    std::fs::write(&tree, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let registry = common::bundled();
    let mut session = Session::recoverable(root.path()).unwrap();
    let setup = common::run_in(&registry, &mut session, |_| {
        json!({
            "operation":"basecraft.delimitation/workspace_tree@1",
            "inputs":{"tree":common::file("tree.nwk")},"parameters":{},
            "destinations":{"workspace":"species.spart"}
        })
    });
    common::assert_completed(&setup);
    let mut jobs = JobManager::default();
    let accepted = common::submit_in(&mut jobs, &registry, &mut session, |_| json!({
        "operation":"basecraft.delimitation/manual_cut@1", "inputs":{"tree":common::file("tree.nwk")},
        "parameters":{"rooted":true,"threshold":0.25},"target":{"kind":"file","path":"species.spart"}
    })).unwrap();
    let completed = common::wait_in(&jobs, &mut session, &accepted.run_id);
    common::assert_completed(&completed);
    let target = jobs.target_document(&accepted.run_id).unwrap();
    let count = session.delimitation(target).unwrap().hypotheses.len();
    assert!(count > 0);
    drop(jobs);
    drop(session);
    assert_reopened_completion(&root.path().join("species.spart"), &accepted.run_id, count);
}

#[test]
fn genuine_workspace_edit_keeps_new_results_as_artifacts_and_records_completion() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree.nwk");
    std::fs::write(&tree, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let registry = common::bundled();
    let mut session = Session::new(root.path());
    let input = session.open(&tree).unwrap();
    let setup = common::run_in(&registry, &mut session, |s| {
        json!({
            "operation":"basecraft.delimitation/workspace_tree@1", "inputs":{"tree":common::document(s, input)},
            "parameters":{},"destinations":{"workspace":"species.spart"}
        })
    });
    common::assert_completed(&setup);
    let doc = session.open(&root.path().join("species.spart")).unwrap();
    let mut jobs = JobManager::default();
    let accepted = common::submit_in(&mut jobs, &registry, &mut session, |s| json!({
        "operation":"basecraft.delimitation/manual_cut@1", "inputs":{"tree":common::file("tree.nwk")},
        "parameters":{"rooted":true,"threshold":0.25},"target":common::document(s, doc)[0]
    })).unwrap();
    let view = session.delimitation(doc).unwrap().view.clone();
    session
        .apply(
            doc,
            basecraft_protocol::Op::DelimitationEdit {
                edit: Box::new(
                    basecraft_protocol::delimitation::DelimitationEdit::SetView { view },
                ),
            },
        )
        .unwrap();
    let outcome = common::wait_in(&jobs, &mut session, &accepted.run_id);
    common::assert_completed(&outcome);
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| warning.contains("Workspace changed"))
    );
    assert_eq!(session.delimitation(doc).unwrap().hypotheses, Vec::new());
    assert!(jobs.finalized_generation(&accepted.run_id).is_none());
    assert!(common::artifact(&outcome, "result.json").is_file());
    assert!(
        session
            .document_record(doc)
            .unwrap()
            .events
            .iter()
            .any(|event| matches!(&event.op,
        basecraft_protocol::Op::JobFinished { report } if report.id == outcome.run_id
            && report.status == basecraft_protocol::records::JobStatus::Completed))
    );
}

#[test]
fn terminal_report_publication_failure_repairs_without_duplicate_completion_or_data_write() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("tree.nwk"),
        "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n",
    )
    .unwrap();
    let registry = common::bundled();
    let mut session = Session::recoverable(root.path()).unwrap();
    let setup = common::run_in(&registry, &mut session, |_| {
        json!({
            "operation":"basecraft.delimitation/workspace_tree@1",
            "inputs":{"tree":common::file("tree.nwk")},"parameters":{},
            "destinations":{"workspace":"species.spart"}
        })
    });
    common::assert_completed(&setup);
    let workspace = root.path().join("species.spart");
    let before = std::fs::read(&workspace).unwrap();
    let metadata = std::fs::metadata(&workspace).unwrap();
    let bundle = records::bundle_for_result(&workspace, None);
    let authored = bundle.join("methods.md");
    std::fs::write(&authored, "User-authored Methods.\n").unwrap();
    let fault = bundle.join("reports/history.md");
    std::fs::remove_file(&fault).unwrap();
    std::fs::create_dir(&fault).unwrap();
    let mut jobs = JobManager::default();
    let accepted = common::submit_in(&mut jobs, &registry, &mut session, |_| {
        json!({
            "operation":"basecraft.delimitation/inspect_tree@1",
            "inputs":{"tree":common::file("tree.nwk")},"parameters":{"rooted":true},
            "target":{"kind":"file","path":"species.spart"}
        })
    })
    .unwrap();
    let partial = common::wait_in(&jobs, &mut session, &accepted.run_id);
    assert_eq!(partial.state, RuntimeState::Partial, "{partial:#?}");
    let recovery = session.recovery_root().to_path_buf();
    assert!(recovery.join("pending-save.json").is_file());
    assert_eq!(std::fs::read(&workspace).unwrap(), before);
    std::fs::remove_dir(&fault).unwrap();
    assert_eq!(
        jobs.repair(&accepted.run_id).unwrap().state,
        RuntimeState::Finalizing
    );
    common::assert_completed(&common::wait_in(&jobs, &mut session, &accepted.run_id));
    let target = jobs.target_document(&accepted.run_id).unwrap();
    assert_eq!(common::tool_run_events(&session, target).len(), 1);
    assert_eq!(std::fs::read(&workspace).unwrap(), before);
    assert_eq!(
        std::fs::metadata(&workspace).unwrap().modified().unwrap(),
        metadata.modified().unwrap(),
        "history-only save must not rewrite unchanged workspace bytes"
    );
    assert_eq!(
        std::fs::read_to_string(&authored).unwrap(),
        "User-authored Methods.\n"
    );
    drop(jobs);
    drop(session);
    assert!(
        !recovery.exists(),
        "success clears completed temporary recovery"
    );
    assert_reopened_completion(&workspace, &accepted.run_id, 0);
}
