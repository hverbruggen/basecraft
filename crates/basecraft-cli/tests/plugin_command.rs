//! `basecraft plugin` resolves registered sequence operations: same registry
//! state, descriptor validation and job runner as `operations run`.
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

/// Every run directory below `root` that holds an input evidence record.
fn input_records(root: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            input_records(&path, found);
        } else if path.file_name().is_some_and(|name| name == "input.json") {
            found.push(root.to_path_buf());
        }
    }
}

fn failure(output: &Output) -> String {
    assert!(!output.status.success(), "unexpected success");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn plugin_command_runs_registered_sequence_operations() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let package = temp.path().join("package");
    std::fs::create_dir(&package).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/registry-sequence-plugin");
    for entry in std::fs::read_dir(&fixture).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), package.join(entry.file_name())).unwrap();
    }
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let sequence = project.join("sequence.fa");
    std::fs::write(&sequence, ">sequence\nACGTACGT\n").unwrap();
    let call = |params: &[&str]| {
        let mut args = vec![
            "plugin",
            package.to_str().unwrap(),
            sequence.to_str().unwrap(),
            "annotate",
        ];
        args.extend_from_slice(params);
        Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args(args)
            .env("BASECRAFT_PROFILE_DIR", &profile)
            .env("BASECRAFT_PLUGINS", temp.path().join("no-bundle"))
            .output()
            .unwrap()
    };
    let error = failure(&call(&["end=4"]));
    assert!(error.contains("register-editable"), "{error}");
    let registered = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["plugins", "register-editable", package.to_str().unwrap()])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .env("BASECRAFT_PLUGINS", temp.path().join("no-bundle"))
        .output()
        .unwrap();
    assert!(
        registered.status.success(),
        "{}",
        String::from_utf8_lossy(&registered.stderr)
    );
    let completed = call(&["end=4"]);
    assert!(
        completed.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&completed.stdout),
        String::from_utf8_lossy(&completed.stderr)
    );
    let outcome: Value = serde_json::from_slice(&completed.stdout).unwrap();
    assert_eq!(outcome["state"], "completed", "{outcome}");
    assert_eq!(
        outcome["operation"],
        "org.example.registry-sequence/annotate@1"
    );
    assert_eq!(outcome["proposals"]["applied"], 1, "{outcome}");
    assert_eq!(
        outcome["effective_parameters"],
        serde_json::json!({"start":0,"end":4,"slow":false})
    );
    // Every later save scans the registered stores, including the records the
    // run above wrote. An unrelated file saves normally.
    let other = project.join("other.fa");
    std::fs::write(&other, ">other\nACGT\n").unwrap();
    let edited = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args([
            "edit",
            other.to_str().unwrap(),
            r#"{"op":"seq.replace","params":{"start":0,"end":1,"residues":"T"}}"#,
        ])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .output()
        .unwrap();
    assert!(
        edited.status.success(),
        "{}",
        String::from_utf8_lossy(&edited.stderr)
    );
    assert_eq!(std::fs::read(&other).unwrap(), b">other\nTCGT\n");
    // The run's own input is the document it edits, so saving the proposals
    // needs no evidence decision.
    let saved = call(&["end=4", "--save"]);
    assert!(
        saved.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&saved.stdout),
        String::from_utf8_lossy(&saved.stderr)
    );
    let gff = std::fs::read_to_string(project.join("sequence.gff3")).unwrap();
    assert!(gff.contains("\t1\t4\t"), "{gff}");
    // The frozen sequence file is recorded as an unchanged (identity) read that
    // the shared evidence reader accepts; the session snapshot is generated.
    let mut records = Vec::new();
    input_records(temp.path(), &mut records);
    let mut transformations = Vec::new();
    let mut saw_proposal_target = false;
    let mut saw_retained_snapshot = false;
    for run in &records {
        let input = basecraft_ops::input_evidence::read_input(run)
            .unwrap_or_else(|error| panic!("{}: {error}", run.display()));
        if let (Some(source), "identity-v1") = (&input.source, input.transformation.as_str()) {
            assert_eq!(input.submitted_hash, source.hash, "{}", run.display());
            let path = run.join(&source.path).canonicalize().unwrap();
            assert_eq!(basecraft_io::hash_file(&path).unwrap(), source.hash);
            let target = input.proposal_target;
            if path == sequence.canonicalize().unwrap() {
                // The editable source is exempt from guarding its own save.
                assert_eq!(target, Some(true), "{}", run.display());
                saw_proposal_target = true;
            } else {
                // Explicit contribution ancestry refers to the immutable
                // submitted snapshot, not another editable proposal target.
                assert_eq!(path.file_name().unwrap(), "snapshot.arrow-pair");
                assert_eq!(target, None, "{}", run.display());
                saw_retained_snapshot = true;
            }
        }
        transformations.push((input.state, input.transformation));
    }
    assert!(
        transformations.contains(&("referenced".into(), "identity-v1".into())),
        "{transformations:?}"
    );
    assert!(
        transformations.contains(&("generated".into(), "sequence-arrow-buffers-v1".into())),
        "{transformations:?}"
    );
    assert!(saw_proposal_target && saw_retained_snapshot);
    let rejected = call(&["bogus=1"]);
    assert!(!rejected.status.success());
    let outcome: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(
        outcome["errors"][0]["code"], "invalid_arguments",
        "{outcome}"
    );
    assert_eq!(std::fs::read(&sequence).unwrap(), b">sequence\nACGTACGT\n");
}

/// A file another analysis read still guards saves; `--input-conflict` passes
/// the same decision the application asks the user for.
#[test]
fn plugin_save_honours_the_input_conflict_decision() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures");
    let package = temp.path().join("package");
    std::fs::create_dir(&package).unwrap();
    for entry in std::fs::read_dir(fixtures.join("registry-sequence-plugin")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), package.join(entry.file_name())).unwrap();
    }
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let sequence = project.join("sequence.fa");
    std::fs::write(&sequence, ">sequence\nACGTACGT\n").unwrap();
    let basecraft = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args(args)
            .env("BASECRAFT_PROFILE_DIR", &profile)
            .env("BASECRAFT_PLUGINS", temp.path().join("no-bundle"))
            .output()
            .unwrap()
    };
    let ok = |output: Output| {
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    ok(basecraft(&[
        "plugins",
        "register-editable",
        package.to_str().unwrap(),
    ]));
    let counter = fixtures.join("registry-plugin");
    ok(basecraft(&[
        "plugins",
        "install",
        counter.to_str().unwrap(),
    ]));
    // An ordinary analysis that read sequence.fa.
    let request = temp.path().join("request.json");
    std::fs::write(
        &request,
        serde_json::json!({"operation":"org.example.registry-fixture/count@1",
            "inputs":{"sources":[{"kind":"file","path":"sequence.fa"}]},
            "parameters":{"mode":"normal"},"destinations":{"report":"count.json"}})
        .to_string(),
    )
    .unwrap();
    let counted = ok(basecraft(&[
        "operations",
        "run",
        "org.example.registry-fixture/count@1",
        "--project",
        project.to_str().unwrap(),
        "--request",
        request.to_str().unwrap(),
    ]));
    let counted: Value = serde_json::from_slice(&counted.stdout).unwrap();
    assert_eq!(counted["state"], "completed", "{counted}");
    let plugin = |extra: &[&str]| {
        let mut args = vec![
            "plugin",
            package.to_str().unwrap(),
            sequence.to_str().unwrap(),
            "annotate",
            "end=4",
            "--save",
        ];
        args.extend_from_slice(extra);
        basecraft(&args)
    };
    // Default and explicit cancel refuse, naming the conflict; nothing is saved.
    for extra in [&[][..], &["--input-conflict", "cancel"][..]] {
        let error = failure(&plugin(extra));
        assert!(error.contains("analysis_input_conflict"), "{error}");
        assert!(!project.join("sequence.gff3").exists());
    }
    let error = failure(&plugin(&["--input-conflict", "keep"]));
    assert!(error.contains("preserve_copy"), "{error}");
    // --input-conflict needs --save.
    let error = failure(&basecraft(&[
        "plugin",
        package.to_str().unwrap(),
        sequence.to_str().unwrap(),
        "annotate",
        "--input-conflict",
        "cancel",
    ]));
    assert!(error.contains("--save"), "{error}");
    // Preserving a copy keeps the earlier analysis verifiable and saves.
    ok(plugin(&["--input-conflict", "preserve_copy"]));
    assert!(project.join("sequence.gff3").exists());
    ok(basecraft(&[
        "provenance",
        "verify",
        project.join("count.json").to_str().unwrap(),
    ]));
}
