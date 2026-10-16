use std::process::Command;
#[test]
fn cli_cache_matrix_tree_and_groups() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    std::fs::write(
        p.join("input.fa"),
        ">a\nAAAAAAAC\n>b\nAAAAAACC\n>c\nCCAAAAAC\n>d\nCCAAAACC\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_basecraft-distance"))
            .current_dir(p)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let initial = run(&[
        "--input",
        "input.fa",
        "--model",
        "jc69",
        "--cache",
        "cache.json",
        "--tree",
        "tree.nwk",
        "--matrix",
        "matrix.tsv",
        "--threads",
        "2",
    ]);
    assert_eq!(initial["cache_reused"], false);
    assert_eq!(initial["model"], "jc69");
    let reused = run(&[
        "--input",
        "input.fa",
        "--model",
        "jc69",
        "--cache",
        "cache.json",
    ]);
    assert_eq!(reused["cache_reused"], true);
    std::fs::write(
        p.join("groups.json"),
        r#"{"a":"one","b":"one","c":"two","d":"two"}"#,
    )
    .unwrap();
    let summary = run(&[
        "--cache",
        "cache.json",
        "--groups",
        "groups.json",
        "--summary",
        "summary.json",
    ]);
    assert_eq!(summary["model"], "jc69");
    assert!(p.join("summary.json").exists());
    let changed = run(&[
        "--input",
        "input.fa",
        "--model",
        "p",
        "--cache",
        "cache.json",
    ]);
    assert_eq!(changed["cache_reused"], false);
    let bad = Command::new(env!("CARGO_BIN_EXE_basecraft-distance"))
        .current_dir(p)
        .args(["--input", "input.fa", "--matrix", "input.fa"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
}

#[test]
fn bootstrap_cli_reports_success_failures_and_cache_requirements() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    std::fs::write(p.join("input.fa"), ">a\nAAAAAAAAAACCAAAACCAA\n>b\nAAAAAAAAAACCAAAACCAA\n>c\nAAAAGGAAAACCAAAACCAA\n>d\nAAAAGGAAAACCAAAACCAA\n").unwrap();
    let binary = env!("CARGO_BIN_EXE_basecraft-distance");
    for method in ["sorted", "upgma"] {
        let result = Command::new(binary)
            .current_dir(p)
            .args([
                "--input",
                "input.fa",
                "--model",
                "p",
                "--tree",
                "tree.nwk",
                "--cache",
                "cache.json",
                "--bootstrap",
                "64",
                "--seed",
                "17",
                "--method",
                method,
                "--progress",
                "progress.json",
                "--report",
                "run.json",
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let metadata: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(metadata["bootstrap"]["requested"], 64);
        assert_eq!(metadata["bootstrap"]["completed"], 64);
        assert_eq!(metadata["bootstrap"]["seed"], 17);
        assert_eq!(metadata["bootstrap"]["support_withheld"], false);
        let progress: serde_json::Value =
            serde_json::from_slice(&std::fs::read(p.join("progress.json")).unwrap()).unwrap();
        assert_eq!(metadata["bootstrap"], progress);
    }
    for args in [
        vec![
            "--cache",
            "cache.json",
            "--tree",
            "missing.nwk",
            "--bootstrap",
            "8",
        ],
        vec![
            "--input",
            "input.fa",
            "--tree",
            "missing.nwk",
            "--bootstrap",
            "8",
            "--method",
            "njstar",
        ],
        vec![
            "--input",
            "input.fa",
            "--matrix",
            "matrix.tsv",
            "--bootstrap",
            "8",
        ],
        vec![
            "--input",
            "input.fa",
            "--tree",
            "missing.nwk",
            "--bootstrap",
            "8",
            "--progress",
            "input.fa",
        ],
    ] {
        let result = Command::new(binary)
            .current_dir(p)
            .args(args)
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
    std::fs::write(p.join("bad.fa"), ">a\nAAAA\n>b\nGGGG\n").unwrap();
    let result = Command::new(binary)
        .current_dir(p)
        .args([
            "--input",
            "bad.fa",
            "--model",
            "k2p",
            "--tree",
            "failed.nwk",
            "--bootstrap",
            "8",
            "--report",
            "failure.json",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!p.join("failed.nwk").exists());
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p.join("failure.json")).unwrap()).unwrap();
    assert_eq!(report["bootstrap"]["attempted"], 0);
    assert!(
        report["bootstrap"]["failures"][0]
            .as_str()
            .unwrap()
            .contains("saturation")
    );
    std::fs::write(p.join("gaps.fa"), ">a\nA---\n>b\nAAAA\n").unwrap();
    let result = Command::new(binary)
        .current_dir(p)
        .args([
            "--input",
            "gaps.fa",
            "--model",
            "p",
            "--tree",
            "withheld.nwk",
            "--bootstrap",
            "64",
            "--report",
            "withheld.json",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["bootstrap"]["support_withheld"], true);
    assert!(report["bootstrap"]["failed_replicates"].as_u64().unwrap() > 0);
    assert!(String::from_utf8_lossy(&result.stderr).contains("support withheld"));
}

#[test]
fn retained_collection_cli_discovery_collisions_and_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let input = ">a\nAAAAAC\n>b\nAAAAAT\n>c\nACAAAC\n";
    std::fs::write(p.join("input.fa"), input).unwrap();
    let binary = env!("CARGO_BIN_EXE_basecraft-distance");
    let run = |path: &str, extra: &[&str]| {
        Command::new(binary)
            .current_dir(p)
            .args([
                "--input",
                "input.fa",
                "--model",
                "p",
                "--tree",
                "tree.nwk",
                "--bootstrap",
                "16",
                "--bootstrap-trees",
                path,
            ])
            .args(extra)
            .output()
            .unwrap()
    };
    let result = run("replicates.nex", &["--report", "run.json"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p.join("run.json")).unwrap()).unwrap();
    let artifact = &report["bootstrap"]["replicate_trees"];
    assert_eq!(artifact["status"], "complete");
    let retained = std::fs::read_to_string(p.join(artifact["path"].as_str().unwrap())).unwrap();
    assert_eq!(
        basecraft_io::newick::parse_trees(&retained).unwrap().len(),
        16
    );
    assert!(!run("replicates.nex", &[]).status.success());
    assert_eq!(
        std::fs::read_to_string(p.join("replicates.nex")).unwrap(),
        retained
    );
    for path in ["input.fa", "tree.nwk", "missing/trees.nex"] {
        assert!(!run(path, &[]).status.success());
    }
    assert!(
        !run("status.nex", &["--progress", "status.partial.nex"])
            .status
            .success()
    );
    assert_eq!(std::fs::read_to_string(p.join("input.fa")).unwrap(), input);
    std::fs::write(p.join("cancel"), "").unwrap();
    let result = run("cancelled.nex", &["--cancel-file", "cancel"]);
    assert!(result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["bootstrap"]["cancelled"], true);
    assert_eq!(
        report["bootstrap"]["replicate_trees"]["status"],
        "cancelled"
    );
    assert!(!p.join("cancelled.nex").exists());
    assert!(p.join("cancelled.partial.nex").exists());
    let disabled = Command::new(binary)
        .current_dir(p)
        .args([
            "--input",
            "input.fa",
            "--tree",
            "tree.nwk",
            "--bootstrap-trees",
            "disabled.nex",
        ])
        .output()
        .unwrap();
    assert!(!disabled.status.success());
    assert!(!p.join("disabled.nex").exists());
}

#[test]
fn python_plugin_retention_contract_with_native_engine() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let python = std::env::var_os("BASECRAFT_PYTHON").unwrap_or_else(|| "python3".into());
    let output = Command::new(python)
        .current_dir(&repo)
        .env("PYTHONPATH", repo.join("python/src"))
        .env(
            "BASECRAFT_DISTANCE",
            env!("CARGO_BIN_EXE_basecraft-distance"),
        )
        .args(["-m", "unittest", "discover", "-s", "plugins/distance", "-v"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn late_output_errors_keep_structured_publication_evidence() {
    for output in ["--tree", "--matrix", "--report", "--summary", "--progress"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let input = if output == "--progress" {
            ">a\nAAA---\n>b\n---AAA\n"
        } else {
            ">a\nAAAAAC\n>b\nAAAAAT\n>c\nACAAAC\n"
        };
        std::fs::write(root.join("input.fa"), input).unwrap();
        std::fs::write(root.join("groups.json"), r#"{"a":"one","b":"one"}"#).unwrap();
        std::fs::create_dir(root.join("blocked")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_basecraft-distance"));
        command.current_dir(root).args([
            "--input",
            "input.fa",
            "--model",
            "p",
            "--bootstrap",
            "4",
            "--bootstrap-trees",
            "trees.nex",
            "--threads",
            "1",
        ]);
        if output != "--tree" {
            command.args(["--tree", "primary.nwk"]);
        }
        if output == "--summary" {
            command.args(["--groups", "groups.json"]);
        }
        let result = command.args([output, "blocked"]).output().unwrap();
        assert!(!result.status.success(), "{output}");
        let metadata: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(metadata["output_status"], "failed");
        assert!(metadata["error"].as_str().is_some_and(|s| !s.is_empty()));
        let collection = &metadata["bootstrap"]["replicate_trees"];
        let partial = output == "--progress";
        assert_eq!(
            collection["path"],
            if partial {
                "trees.partial.nex"
            } else {
                "trees.nex"
            }
        );
        assert_eq!(
            collection["status"],
            if partial {
                "original_failed"
            } else {
                "complete"
            }
        );
        assert_eq!(collection["trees"], if partial { 0 } else { 4 });
        assert!(root.join(collection["path"].as_str().unwrap()).is_file());
    }
}
