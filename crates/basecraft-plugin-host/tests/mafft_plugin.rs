//! End to end: realign an alignment with the MAFFT plugin and log the run.
//! Skips when the plugin's Python or MAFFT isn't available.

use std::fmt::Write as _;
use std::path::Path;

use basecraft_ops::Session;
use basecraft_plugin_host::FileJob;
use basecraft_plugin_host::file_tool::{AlignmentInput, output_beside};
use basecraft_plugin_host::process::python_interpreter;
use basecraft_protocol::{Actor, Op};

fn mafft_available() -> bool {
    let python = python_interpreter();
    let script = "import sys, shutil, pathlib; \
        sys.exit(0 if (pathlib.Path(sys.prefix) / 'bin' / 'mafft').exists() or shutil.which('mafft') else 1)";
    let ok = std::process::Command::new(&python)
        .args(["-c", script])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipping: no MAFFT beside {}", python.display());
    }
    ok
}

#[test]
fn realigns_with_mafft_and_logs_the_run() {
    if !mafft_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Unaligned, different lengths: opens as rows padded with gaps.
    let seqs = [
        ("a", "ATGGCTAAAGGTCCTGAAGCTTTAGAA"),
        ("b", "ATGGCTAAGGTCCTGAAGCTTTAGAA"),
        ("c", "ATGGCTAAAGGTCCTGAAGCTTTTAGAA"),
    ];
    let mut text = String::new();
    for (name, seq) in seqs {
        let _ = writeln!(text, ">{name}\n{seq}");
    }
    let file = root.join("seqs.fasta");
    std::fs::write(&file, text).unwrap();

    let mut session = Session::new(root);
    let doc = session.open(&file).unwrap();
    assert!(session.is_alignment(doc));
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/mafft");
    let output = output_beside(&file, "mafft", "fasta");
    let job = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "align"),
        AlignmentInput::Ungapped,
        output.clone(),
    )
    .unwrap();
    let result = job.run().unwrap();
    job.record(&mut session, doc, &result).unwrap();
    assert_eq!(result.tool.as_ref().map(|t| t.name.as_str()), Some("mafft"));

    // The output is an alignment of the same sequences.
    let out = session.open(&output).unwrap();
    let aln = session.alignment(out);
    assert_eq!(aln.row_count(), 3);
    for (i, (name, seq)) in seqs.iter().enumerate() {
        assert_eq!(aln.row(i).name, *name);
        let row = aln.read(i, 0, aln.width());
        let ungapped: Vec<u8> = row.into_iter().filter(|&b| b != b'-').collect();
        assert_eq!(ungapped, seq.as_bytes(), "row {name}");
    }
    drop(session);

    let entries = basecraft_ops::log::read_all(root).unwrap();
    let run = entries
        .iter()
        .find(|e| matches!(e.op, Op::ToolRun { .. }))
        .expect("tool.run logged");
    assert!(matches!(&run.actor, Actor::Tool { tool, .. } if tool == "mafft"));
    assert_eq!(run.result.hashes.len(), 2, "input and output hashed");
    let report = basecraft_ops::replay::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}
