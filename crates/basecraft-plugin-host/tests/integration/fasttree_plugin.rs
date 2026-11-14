//! End to end: infer a tree from an alignment with the registered `FastTree`
//! operation, open it, and record the run. Skips when `FastTree` isn't available.
use crate::common;
use basecraft_ops::records::AlignmentLinkExt;

use std::fmt::Write as _;

use basecraft_ops::Session;
use basecraft_plugin_host::file_tool::output_beside;
use basecraft_plugin_host::process::python_interpreter;
use basecraft_protocol::Actor;
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

/// Write `NAME.aln.fasta`, open it and run `FastTree` on the open document,
/// exporting `NAME.fasttree.nwk`.
fn run_fasttree(
    root: &std::path::Path,
    name: &str,
    fasta: &str,
) -> (
    Session,
    basecraft_ops::DocId,
    basecraft_protocol::operation::OperationOutcome,
) {
    let file = root.join(format!("{name}.aln.fasta"));
    std::fs::write(&file, fasta).unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&file).unwrap();
    let destination = format!("{name}.fasttree.nwk");
    let outcome = common::run_in(&common::bundled(), &mut session, |s| {
        json!({"operation":"basecraft.fasttree/tree@1","inputs":{"alignment":common::document(s, doc)},
            "parameters":{},"destinations":{"artifacts/tree.nwk":destination}})
    });
    common::assert_completed(&outcome);
    (session, doc, outcome)
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
    let output = output_beside(&file, "fasttree", "nwk");
    assert_eq!(output, root.join("genes.fasttree.nwk"));
    let (mut session, doc, outcome) = run_fasttree(root, "genes", &text);
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
    assert!(link.checked_alignment(None, &output, t).is_ok());

    let runs = common::tool_run_events(&session, doc);
    assert_eq!(runs.len(), 1);
    assert!(matches!(&runs[0].actor, Actor::Tool { tool, .. } if tool == "fasttree"));
    assert!(runs[0].op.is_run(&outcome.run_id));
}

/// A DNA alignment with one stray X still gets GTR: basecraft's guess (D12)
/// ignores X, as `FastTree`'s own rule did.
#[test]
fn a_dna_alignment_with_one_stray_letter_still_gets_gtr() {
    crate::isolated_profile::isolate!();
    assert!(fasttree_available(), "FastTree is needed");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (_, _, outcome) = run_fasttree(
        root,
        "stray",
        ">a\nATGGCTAAAGGTCCTGAAGCTTTAGAAGCT\n>b\nATGGCTAAAGGTCCAGAAGCTTTAGAAGCA\n\
         >c\nATGGCAAAAGGACCAGAAGCXTTAGAAGCA\n>d\nATGCCAAAAGGACCAGAAGCATTAGTAGCA\n",
    );
    let execution = common::read_json(&common::evidence(&outcome).join("execution.json"));
    assert_eq!(
        execution["execution"]["resolved_parameters"]["model"], "GTR",
        "{execution}"
    );
}

/// An alignment whose rows are of different kinds is refused before
/// `FastTree` starts, naming the odd row (D12).
#[test]
fn a_mixed_alignment_is_refused_before_launch() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("mixed.aln.fasta");
    std::fs::write(
        &file,
        ">a\nATGGCTAAAGGTCCTGAAGCTTTAGAAGCT\n>b\nATGGCTAAAGGTCCAGAAGCTTTAGAAGCA\n\
         >c\nATGGCAAAAGGACCAGAAGCATTAGAAGCA\n>p\nMKLEFIPQWERTYKLEFIPQWERTYKLEFI\n",
    )
    .unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&file).unwrap();
    let (_, error) = common::refused_in(&common::bundled(), &mut session, |s| {
        json!({"operation":"basecraft.fasttree/tree@1","inputs":{"alignment":common::document(s, doc)},
            "parameters":{},"destinations":{"artifacts/tree.nwk":"mixed.fasttree.nwk"}})
    });
    assert!(
        error.contains("mixed.aln.fasta: The sequences are not all of one kind"),
        "{error}"
    );
    assert!(error.contains("1 protein: p;"), "{error}");
    assert!(!root.join("mixed.fasttree.nwk").exists());
}
