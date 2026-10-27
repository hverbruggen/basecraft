//! Compatibility commands resolve registered operations: same registry state,
//! descriptor validation and job runner as `operations run`.
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run(profile: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .env("BASECRAFT_PROFILE_DIR", profile)
        .output()
        .unwrap()
}

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
fn tool_and_distance_validate_against_registered_descriptors() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let source = project.join("input.afa");
    std::fs::write(&source, ">a\nACGT\n>b\nACGA\n>c\nTCGA\n").unwrap();
    let source = source.to_str().unwrap();
    let output = project.join("out.afa");
    let output = output.to_str().unwrap();
    let tool = |params: &str| {
        run(
            &profile,
            &[
                "tool", "trimal", source, "--output", output, "--params", params,
            ],
        )
    };
    let error = failure(&tool(r#"{"method":"bogus"}"#));
    assert!(error.contains("operation contract"), "{error}");
    let error = failure(&tool(r#"{"cache":"x.json"}"#));
    assert!(error.contains("unknown parameter"), "{error}");
    let error = failure(&run(
        &profile,
        &[
            "tool",
            "delimitation",
            source,
            "--output",
            output,
            "--params",
            "{}",
        ],
    ));
    assert!(error.contains("params.method"), "{error}");
    // The legacy matrix command never had a seed; the descriptor says so.
    let matrix = project.join("matrix.tsv");
    let error = failure(&run(
        &profile,
        &[
            "distance",
            source,
            "--matrix",
            "--output",
            matrix.to_str().unwrap(),
            "--seed",
            "3",
        ],
    ));
    assert!(error.contains("unknown parameter"), "{error}");
    assert!(!matrix.exists());

    // A disabled bundled plugin is refused by both routes with the same code.
    assert!(
        run(&profile, &["plugins", "disable", "basecraft.trimal"])
            .status
            .success()
    );
    let error = failure(&tool("{}"));
    assert!(error.contains("plugin_disabled"), "{error}");
    let request = temp.path().join("request.json");
    std::fs::write(
        &request,
        serde_json::json!({"operation":"basecraft.trimal/trim@1",
            "inputs":{"alignment":[{"kind":"file","path":"input.afa"}]},
            "parameters":{},"destinations":{"artifacts":"trimmed"}})
        .to_string(),
    )
    .unwrap();
    let refused = run(
        &profile,
        &[
            "operations",
            "run",
            "basecraft.trimal/trim@1",
            "--project",
            project.to_str().unwrap(),
            "--request",
            request.to_str().unwrap(),
        ],
    );
    let refused: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(refused["errors"][0]["code"], "plugin_disabled", "{refused}");
    assert!(!Path::new(output).exists());
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
    // Replacing the run's own input asks for an evidence decision, which the
    // CLI save cannot give; the file stays unchanged.
    let saved = call(&["end=4", "--save"]);
    assert!(!saved.status.success());
    let error = String::from_utf8_lossy(&saved.stderr);
    assert!(error.contains("analysis_input_conflict"), "{error}");
    // The frozen sequence file is recorded as an unchanged (identity) read that
    // the shared evidence reader accepts; the session snapshot is generated.
    let mut records = Vec::new();
    input_records(temp.path(), &mut records);
    let mut transformations = Vec::new();
    for run in &records {
        let input = basecraft_ops::input_evidence::read_input(run)
            .unwrap_or_else(|error| panic!("{}: {error}", run.display()));
        if let (Some(source), "identity-v1") = (&input.source, input.transformation.as_str()) {
            assert_eq!(input.submitted_hash, source.hash, "{}", run.display());
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
    let rejected = call(&["bogus=1"]);
    assert!(!rejected.status.success());
    let outcome: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(
        outcome["errors"][0]["code"], "invalid_arguments",
        "{outcome}"
    );
    assert_eq!(std::fs::read(&sequence).unwrap(), b">sequence\nACGTACGT\n");
}
