//! What the command line does with the shared command table: which documents
//! a command saves, the report export (D30), duplicate-name numbering (D29).
use std::path::Path;
use std::process::{Command, Output};

use basecraft_ops::records::{self, ReportExt};
use basecraft_protocol::Op;

fn basecraft(profile: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .env("BASECRAFT_PROFILE_DIR", profile)
        .output()
        .unwrap()
}

fn ok(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// `delimitation combine` edits a copy of its first source in the session and
/// saves it under the new name. The command line saves only the documents a
/// command declares (`Call::edits`), so the sources keep their bytes and their
/// history.
#[test]
fn combine_leaves_its_sources_unchanged() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/delimitation/manual.spart");
    let sources = [project.join("a.spart"), project.join("b.spart")];
    for source in &sources {
        std::fs::copy(&fixture, source).unwrap();
    }
    // Each reading opens the file anew, so compare what its history records,
    // not the reading session's own identifiers and times.
    let state = |path: &Path| {
        let report: serde_json::Value =
            serde_json::from_str(&ok(basecraft(&profile, &["history", text(path), "--json"])))
                .unwrap();
        let rows: Vec<_> = report["history"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| (row["summary"].clone(), row["status"].clone()))
            .collect();
        (std::fs::read(path).unwrap(), rows)
    };
    let before: Vec<_> = sources.iter().map(|s| state(s)).collect();
    let output = project.join("c.spart");
    ok(basecraft(
        &profile,
        &[
            "--project",
            text(&project),
            "delimitation",
            "combine",
            text(&sources[0]),
            text(&sources[1]),
            "--output",
            text(&output),
        ],
    ));
    assert!(output.is_file());
    let after: Vec<_> = sources.iter().map(|s| state(s)).collect();
    assert_eq!(before, after);
}

/// D30: `methods --output` writes the files the History window exports, the
/// same bytes as the window's export of the same saved file.
#[test]
fn methods_output_exports_the_history_windows_files() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let file = temp.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    ok(basecraft(
        &profile,
        &[
            "edit",
            text(&file),
            r#"{"op":"seq.replace","params":{"start":0,"end":1,"residues":"T"}}"#,
        ],
    ));
    let cli = temp.path().join("cli-report");
    ok(basecraft(
        &profile,
        &["methods", text(&file), "--output", text(&cli)],
    ));
    for name in [
        "methods.generated.md",
        "history.md",
        "record.json",
        "parameters.tsv",
        "source-map.json",
        "citations.json",
        "manifest.json",
        "report.json",
    ] {
        assert!(cli.join(name).is_file(), "{name}");
    }
    // The History window's provenance action exports this report.
    let window = temp.path().join("window-report");
    records::DocumentRecord::saved_report(&file, None)
        .unwrap()
        .export(&window)
        .unwrap();
    assert_eq!(
        std::fs::read(cli.join("methods.generated.md")).unwrap(),
        std::fs::read(window.join("methods.generated.md")).unwrap()
    );
}

/// D29: an alignment file is numbered as one edit, saved with that edit in
/// its history, on the command line as in the window.
#[test]
fn numbering_an_alignment_records_the_edit() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let file = temp.path().join("rows.afa");
    std::fs::write(&file, ">a\nACGT\n>a\nACGA\n>b\nACGG\n").unwrap();
    let answer: serde_json::Value = serde_json::from_str(&ok(basecraft(
        &profile,
        &["number-duplicates", text(&file)],
    )))
    .unwrap();
    assert_eq!(answer["names"], serde_json::json!(["a"]));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        ">a.1\nACGT\n>a.2\nACGA\n>b\nACGG\n"
    );
    let events = records::inspect(&file, None).unwrap().events;
    let renames = events
        .iter()
        .filter(|event| matches!(event.op, Op::AlnRenameRow { .. }))
        .count();
    assert_eq!(renames, 2, "{events:?}");
}

/// D29: the records of a GenBank file are renamed in the file, a run its
/// history records.
#[test]
fn numbering_records_records_the_rewrite() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let file = temp.path().join("two.gb");
    let record = "LOCUS       ABC          4 bp    DNA\nORIGIN\n        1 acgt\n//\n";
    std::fs::write(&file, record.repeat(2)).unwrap();
    ok(basecraft(&profile, &["number-duplicates", text(&file)]));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        record.replace("ABC   ", "ABC.1 ") + &record.replace("ABC   ", "ABC.2 ")
    );
    let history = ok(basecraft(&profile, &["history", text(&file), "--json"]));
    assert!(history.contains("number_records"), "{history}");
}

/// `mcp` takes its project as its argument and leaves saves to its client.
#[test]
fn mcp_refuses_the_command_line_options_it_would_ignore() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let refused = basecraft(
        &profile,
        &["--project", text(temp.path()), "mcp", text(temp.path())],
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("mcp does not take --project"));
}
