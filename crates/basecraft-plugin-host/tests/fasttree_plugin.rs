//! End to end: infer a tree from an alignment with the registered `FastTree`
//! operation, open it, and record the run. Skips when `FastTree` isn't available.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::records::AlignmentLinkExt;
mod common;

use std::fmt::Write as _;

use basecraft_ops::Session;
use basecraft_plugin_host::file_tool::output_beside;
use basecraft_plugin_host::process::python_interpreter;
use basecraft_protocol::{Actor, Op};
use serde_json::json;

fn fasttree_available() -> bool {
    let python = python_interpreter();
    let script = "import sys, shutil, pathlib; \
        b = pathlib.Path(sys.prefix) / 'bin'; \
        sys.exit(0 if (b / 'FastTree').exists() or (b / 'fasttree').exists() \
        or shutil.which('FastTree') or shutil.which('fasttree') else 1)";
    let ok = std::process::Command::new(&python)
        .args(["-c", script])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipping: no FastTree beside {}", python.display());
    }
    ok
}

#[test]
fn builds_a_tree_with_fasttree_and_logs_the_run() {
    crate::isolated_profile::isolate!();
    if !fasttree_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Aligned, with a gap, and a name Newick would need to quote.
    let seqs = [
        ("Codium fragile", "ATGGCTAAAGGTCCTGAAGCTTTAGAAGCT"),
        ("b", "ATGGCTAAAGGTCCAGAAGCTTTAGAAGCA"),
        ("c", "ATGGCAAAAGGACCAGAAGC---AGAAGCA"),
        ("d", "ATGCCAAAAGGACCAGAAGCATTAGTAGCA"),
    ];
    let mut text = String::new();
    for (name, seq) in seqs {
        let _ = writeln!(text, ">{}\n{seq}", name.replace(' ', "_"));
    }
    let file = root.join("genes.aln.fasta");
    std::fs::write(&file, text).unwrap();

    let mut session = Session::new(root);
    let doc = session.open(&file).unwrap();
    let output = output_beside(&file, "fasttree", "nwk");
    assert_eq!(output, root.join("genes.fasttree.nwk"));
    let outcome = common::run_in(&common::bundled(), &mut session, |s| {
        json!({"operation":"basecraft.fasttree/tree@1","inputs":{"alignment":common::document(s, doc)},
            "parameters":{},"destinations":{"artifacts/tree.nwk":"genes.fasttree.nwk"}})
    });
    common::assert_completed(&outcome);
    let execution = common::read_json(&common::evidence(&outcome).join("execution.json"));
    assert!(
        execution["execution"].to_string().contains("GTR"),
        "{execution}"
    );

    // The tree opens with every tip and FastTree's local support values.
    let tree = session.open(&output).unwrap();
    let t = session.tree(tree).unwrap();
    assert_eq!(t.tip_count(), 4);
    assert!(t.find_tip("Codium_fragile").is_some());
    assert_eq!(t.supports().len(), 1);
    assert_eq!(t.supports()[0].name, "SH-like local support");
    assert_eq!(
        t.supports()[0].kind,
        basecraft_protocol::SupportKind::Probability
    );
    assert_eq!(t.supports()[0].decimals(t.style()), 2);
    let link = basecraft_ops::records::AlignmentLink::read(&output).unwrap();
    assert!(link.inferred);
    assert_eq!(link.source, file);
    assert!(link.checked_snapshot(&output, t).is_ok());

    let runs = common::tool_run_events(&session, doc);
    assert_eq!(runs.len(), 1);
    assert!(matches!(&runs[0].actor, Actor::Tool { tool, .. } if tool == "fasttree"));
    assert!(
        matches!(&runs[0].op, Op::ToolRun { params, .. } if params["run_id"] == outcome.run_id.as_str())
    );
}
