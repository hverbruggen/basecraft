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
