//! End to end: infer a tree from an alignment with the `FastTree` plugin,
//! open it, and log the run. Skips when `FastTree` isn't available.

use std::fmt::Write as _;
use std::path::Path;

use basecraft_ops::Session;
use basecraft_plugin_host::FileJob;
use basecraft_plugin_host::file_tool::{AlignmentInput, output_beside};
use basecraft_plugin_host::process::python_interpreter;
use basecraft_protocol::{Actor, Op};

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
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/fasttree");
    let output = output_beside(&file, "fasttree", "nwk");
    assert_eq!(output, root.join("genes.fasttree.nwk"));
    let job = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "tree"),
        AlignmentInput::Aligned,
        output.clone(),
    )
    .unwrap();
    let result = job.run().unwrap();
    job.record(&mut session, doc, &result).unwrap();
    assert!(
        result.message.as_deref().unwrap_or("").contains("GTR"),
        "{:?}",
        result.message
    );

    // The tree opens with every tip and FastTree's local support values.
    let tree = session.open(&output).unwrap();
    let t = session.tree(tree);
    assert_eq!(t.tip_count(), 4);
    assert!(t.find_tip("Codium_fragile").is_some());
    assert_eq!(t.supports().len(), 1);
    assert_eq!(t.supports()[0].name, "SH-like local support");
    assert_eq!(
        t.supports()[0].kind,
        basecraft_protocol::SupportKind::Probability
    );
    assert_eq!(t.supports()[0].decimals(t.style()), 2);
    let link = basecraft_ops::links::AlignmentLink::read(&output).unwrap();
    assert!(link.inferred);
    assert_eq!(link.source, file);
    assert!(link.checked_snapshot(&output, t).is_ok());

    let entries = basecraft_ops::log::read_all(root).unwrap();
    let run = entries
        .iter()
        .find(|e| matches!(e.op, Op::ToolRun { .. }))
        .unwrap();
    assert!(matches!(&run.actor, Actor::Tool { tool, .. } if tool == "fasttree"));
}
