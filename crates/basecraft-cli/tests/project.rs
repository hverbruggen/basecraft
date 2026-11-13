//! `--project` decides where a file's history goes; without it files are loose.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::process::Command;

fn edit(args: &[&str], file: &std::path::Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .args([
            "edit",
            file.to_str().unwrap(),
            r#"{"op":"seq.replace","params":{"start":0,"end":1,"residues":"T"}}"#,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn history_goes_to_the_chosen_project_or_beside_a_loose_file() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let chosen = root.path().join("project");
    std::fs::create_dir(&chosen).unwrap();
    let inside = chosen.join("inside.fasta");
    std::fs::write(&inside, b">a\nACGT\n").unwrap();
    edit(&["--project", chosen.to_str().unwrap()], &inside);
    let project = basecraft_ops::project::Project::new(&chosen).unwrap();
    let bundle = project.provenance_bundle(&inside).unwrap();
    assert!(bundle.join("manifest.json").is_file());
    assert!(!basecraft_ops::records::companion(&inside).exists());

    let loose = root.path().join("loose.fasta");
    std::fs::write(&loose, b">a\nACGT\n").unwrap();
    edit(&[], &loose);
    assert!(
        basecraft_ops::records::companion(&loose)
            .join("manifest.json")
            .is_file()
    );
}

#[test]
fn an_edit_where_recovery_cannot_be_kept_is_refused_with_its_code() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    // Another program's `.basecraft` (D13): the edit is refused, the file kept.
    std::fs::write(root.path().join(".basecraft"), b"another program").unwrap();
    let file = root.path().join("gene.fasta");
    std::fs::write(&file, b">a\nACGT\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args([
            "edit",
            file.to_str().unwrap(),
            r#"{"op":"seq.replace","params":{"start":0,"end":1,"residues":"T"}}"#,
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let failure: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    let error = &failure["errors"][0];
    assert_eq!(error["code"], "read_only_folder", "{failure}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains(root.path().to_str().unwrap()), "{message}");
    assert!(message.contains("Copy the file"), "{message}");
    assert_eq!(std::fs::read(&file).unwrap(), b">a\nACGT\n");
}

#[test]
fn a_file_in_a_recent_project_names_that_project_without_choosing_it() {
    use std::io::Write;
    let Some(root) = isolated_profile::enter(
        "a_file_in_a_recent_project_names_that_project_without_choosing_it",
    ) else {
        return;
    };
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    let file = project.join("gene.fasta");
    std::fs::write(&file, b">a\nACGT\n").unwrap();
    let canonical = project.canonicalize().unwrap();
    let list = basecraft_viewmodel::projects::history_file().unwrap();
    std::fs::create_dir_all(list.parent().unwrap()).unwrap();
    std::fs::write(
        &list,
        serde_json::json!({"version": 1, "projects": [canonical]}).to_string(),
    )
    .unwrap();
    let alphabet = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args(args)
            .args(["alphabet", file.to_str().unwrap()])
            .output()
            .unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stderr}");
        stderr
    };
    let note = format!(
        "This file is in the recent project {}.",
        canonical.display()
    );
    assert!(alphabet(&[]).contains(&note));
    assert!(!alphabet(&["--project", project.to_str().unwrap()]).contains("recent project"));

    // MCP, and Python through it, refuse a file outside their project and
    // name the recent project that holds it.
    let other = root.join("other");
    std::fs::create_dir(&other).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["mcp", other.to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in [
        serde_json::json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"open","arguments":{"path":"../project/gene.fasta"}}}),
    ] {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    let reply = String::from_utf8(output.stdout).unwrap();
    assert!(
        reply.contains(&format!(
            "it is in the recent project {}, so start the session there",
            canonical.display()
        )),
        "{reply}"
    );
}
