//! End to end: realign an alignment with the registered MAFFT operation and
//! record the run. Skips when the plugin's Python or MAFFT isn't available.
mod common;
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;

use std::fmt::Write as _;

use basecraft_ops::Session;
use basecraft_plugin_host::file_tool::output_beside;
use basecraft_plugin_host::process::python_interpreter;
use basecraft_protocol::{Actor, Op};
use serde_json::json;

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
    crate::isolated_profile::isolate!();
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
    let output = output_beside(&file, "mafft", "fasta");
    let registry = common::bundled();
    let outcome = common::run_in(&registry, &mut session, |s| {
        json!({"operation":"basecraft.mafft/align@1","inputs":{"alignment":common::document(s, doc)},
            "parameters":{},"destinations":{"artifacts/alignment.afa":"seqs.mafft.fasta"}})
    });
    common::assert_completed(&outcome);
    assert_eq!(output, root.join("seqs.mafft.fasta"));
    // The run's evidence names the external program that did the work.
    let execution = common::read_json(&common::evidence(&outcome).join("execution.json"));
    assert_eq!(execution["executor"], "basecraft.mafft");
    assert_eq!(execution["tool"]["name"], "mafft");

    // The output is an alignment of the same sequences.
    let out = session.open(&output).unwrap();
    let aln = session.alignment(out).unwrap();
    assert_eq!(aln.row_count(), 3);
    for (i, (name, seq)) in seqs.iter().enumerate() {
        assert_eq!(aln.row(i).name, *name);
        let row = aln.read(i, 0, aln.width());
        let ungapped: Vec<u8> = row.into_iter().filter(|&b| b != b'-').collect();
        assert_eq!(ungapped, seq.as_bytes(), "row {name}");
    }
    assert_eq!(
        basecraft_ops::records::verify(&output).unwrap(),
        Vec::<String>::new()
    );
    // The source document's history holds the run, by MAFFT, linked by run ID.
    let runs = common::tool_run_events(&session, doc);
    assert_eq!(runs.len(), 1);
    assert!(matches!(&runs[0].actor, Actor::Tool { tool, .. } if tool == "mafft"));
    let Op::ToolRun { params, .. } = &runs[0].op else {
        unreachable!()
    };
    assert_eq!(params["run_id"], outcome.run_id.as_str());
    assert_eq!(params["operation"], "basecraft.mafft/align@1");
    // The opened result inherits the producing activity, including tool
    // version, execution arguments and citations; reopen/export must retain it.
    let result_record = session.document_record(out).unwrap();
    let producing = result_record
        .events
        .iter()
        .find(|event| {
            matches!(&event.op,
        Op::ToolRun { params, .. } if params["run_id"] == outcome.run_id)
        })
        .unwrap();
    assert_eq!(producing.actor, runs[0].actor);
    let methods = result_record.report().unwrap();
    assert!(methods.methods.to_lowercase().contains("mafft"));
    assert_ne!(methods.citations, Vec::<String>::new());
    let bundle = common::evidence(&outcome);
    let methods_path = bundle.join("methods.md");
    std::fs::write(&methods_path, "User-authored explanation.\n").unwrap();
    let publication = basecraft_ops::records::inspect(&output).unwrap();
    basecraft_ops::records::publish(&bundle, &publication).unwrap();
    assert_eq!(
        std::fs::read_to_string(&methods_path).unwrap(),
        "User-authored explanation.\n"
    );
    let reopened = basecraft_ops::records::DocumentRecord::from_file(&output).unwrap();
    assert!(
        reopened
            .report()
            .unwrap()
            .methods
            .to_lowercase()
            .contains("mafft")
    );
    drop(session);

    let report = basecraft_ops::records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}
