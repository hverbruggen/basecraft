//! Real local/pixi tool runs through the same registered route as the GUI.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::records::AlignmentLinkExt;
mod common;

use basecraft_ops::Session;
use basecraft_plugin_host::process::python_interpreter;
use basecraft_protocol::Op;
use serde_json::json;

fn available(name: &str) -> bool {
    std::process::Command::new(python_interpreter()).args(["-c", &format!(
        "import shutil,sys,pathlib; sys.exit(0 if (pathlib.Path(sys.prefix)/'bin'/'{name}').exists() or shutil.which('{name}') else 1)")])
        .status().is_ok_and(|s|s.success())
}

#[test]
fn trimal_and_iqtree_preserve_identifiers_and_provenance() {
    crate::isolated_profile::isolate!();
    if !available("trimal") || !available("iqtree") {
        eprintln!("Skipping: install pixi bio tools");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = root.join("input.afa");
    std::fs::write(&input, ">a:one\nACGTACGTACGT--ACGTACGTACGT\n>b\nACGTACGTTCGT--ACGTACGTACGT\n>c\nTCGTACGTACGT--ACGTACGTACGA\n>d\nTCGTACGTTCGT--ACGTACGTACGA\n").unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    let registry = common::bundled();
    let trimmed = root.join("trimmed.afa");
    let outcome = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.trimal/trim@1","inputs":{"alignment":common::document(s, doc)},
            "parameters":{},"destinations":{"artifacts/trimmed.afa":"trimmed.afa"}})
    });
    common::assert_completed(&outcome);
    let out = session.open(&trimmed).unwrap();
    assert_eq!(session.alignment(out).unwrap().width(), 24);
    assert_eq!(session.alignment(out).unwrap().row(0).name, "a:one");
    session
        .apply(
            doc,
            Op::AlnReplace {
                row: 0,
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    let tree = root.join("tree.nwk");
    let outcome = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.iqtree/tree@1","inputs":{"alignment":common::document(s, doc)},
            "parameters":{"model":"JC","threads":1},"destinations":{"artifacts/tree.nwk":"tree.nwk"}})
    });
    common::assert_completed(&outcome);
    let tree_doc = session.open(&tree).unwrap();
    assert!(session.tree(tree_doc).unwrap().find_tip("a:one").is_some());
    // The unsaved edit is what IQ-TREE saw, and the tree links to exactly that.
    let link = basecraft_ops::records::AlignmentLink::read(&tree).unwrap();
    assert!(link.inferred);
    assert_eq!(
        link.alignment_version,
        session.alignment(doc).unwrap().version()
    );
    let snapshot = link
        .checked_snapshot(&tree, session.tree(tree_doc).unwrap())
        .unwrap();
    let snapshot_doc = session.open(&snapshot).unwrap();
    assert_eq!(session.alignment(snapshot_doc).unwrap().read(0, 0, 1), b"T");
    // IQ-TREE's report stays with the run's retained results.
    assert!(
        outcome
            .artifacts
            .iter()
            .any(|artifact| artifact.path.ends_with(".iqtree")),
        "{outcome:#?}"
    );
    assert_eq!(common::tool_runs(&session, doc).len(), 2);
    session.flush().unwrap();
    assert!(basecraft_ops::records::replay(root).unwrap().ok());
}
