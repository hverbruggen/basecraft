//! Faults after visible publication must resume owned work without rerunning tools.
mod common;

use basecraft_ops::{Session, provenance};
use basecraft_plugin_host::{jobs::JobManager, policy::SessionPolicy};
use basecraft_protocol::operation::RuntimeState;
use serde_json::json;

const ALIGNMENT: &str = ">a first\nACGT\n>b second\nAC-T\n";
const TREE_FIXTURE: &str = r#"
import time
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin("basecraft.fasttree", "test")
@plugin.context_operation("tree")
def tree(_snapshot, _parameters, context):
    time.sleep(0.3)
    root = Path(context["outputs"]["artifacts"])
    (root / "tree.nwk").write_text("(a:0.1,b:0.2);\n")
    return {"artifacts":[{"role":"artifacts","path":"tree.nwk"}]}
plugin.run()
"#;

#[test]
fn member_metadata_repair_requires_owned_bytes_and_retries_companions() {
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
            assert!(basecraft_ops::links::sidecar(&output).is_file());
            assert!(provenance::verify(&output).unwrap().is_empty());
        }
    }
}

#[test]
fn workspace_inventory_survives_repair_and_rejects_changed_export() {
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
            let publication = provenance::inspect(&output).unwrap();
            assert!(
                publication
                    .outputs
                    .iter()
                    .any(|artifact| artifact.logical_name == "species.spart")
            );
            assert!(provenance::verify(&output).unwrap().is_empty());
        }
    }
}
