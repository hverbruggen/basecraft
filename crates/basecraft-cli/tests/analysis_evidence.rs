//! CLI routes use the same host allocation and shared guarded save as the GUI.
use std::process::Command;

#[test]
fn managed_bootstrap_destinations_and_structured_save_conflict() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    let source = root.path().join("input.afa");
    std::fs::write(
        &source,
        ">a\nAAAAAACCAA\n>b\nAAAAAACCAA\n>c\nAAGGAACCAA\n>d\nAAGGAACCAA\n",
    )
    .unwrap();
    let original = std::fs::read(&source).unwrap();
    let binary = env!("CARGO_BIN_EXE_basecraft");
    for destination in ["metadata", "working"] {
        let output = root.path().join(format!("{destination}.nwk"));
        let result = Command::new(binary)
            .args([
                "distance",
                source.to_str().unwrap(),
                "--output",
                output.to_str().unwrap(),
                "--model",
                "p",
                "--method",
                "sorted",
                "--bootstrap",
                "8",
                "--bootstrap-destination",
                destination,
                "--threads",
                "1",
            ])
            .env("BASECRAFT_PROFILE_DIR", &profile)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let run = basecraft_ops::analysis_store::ArtifactRun::for_result(&output)
            .unwrap()
            .unwrap();
        let filename = format!(
            "bootstrap-{}.nex",
            run.file_name().unwrap().to_string_lossy()
        );
        let collection = if destination == "metadata" {
            run.join(filename)
        } else {
            root.path().join(filename)
        };
        assert!(String::from_utf8_lossy(&result.stdout).contains(collection.to_str().unwrap()));
        assert!(collection.is_file());
        if destination == "metadata" {
            assert!(collection.starts_with(root.path().join(".basecraft/analyses-v1/runs")));
        } else {
            assert_eq!(collection.parent(), Some(root.path()));
        }
        let reader = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/verify_provenance.py");
        let inspected = Command::new("python3")
            .arg(reader)
            .arg(&output)
            .args(["--metadata-only"])
            .output()
            .unwrap();
        assert!(
            inspected.status.success(),
            "{}",
            String::from_utf8_lossy(&inspected.stderr)
        );
        assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".alignment.afa")
        }));
        let verified = Command::new(binary)
            .args(["provenance", "verify", output.to_str().unwrap()])
            .env("BASECRAFT_PROFILE_DIR", &profile)
            .output()
            .unwrap();
        assert!(
            verified.status.success(),
            "{}",
            String::from_utf8_lossy(&verified.stderr)
        );
    }
    let operation = r#"{"op":"aln.replace","params":{"row":0,"start":0,"end":1,"residues":"T"}}"#;
    let refused = Command::new(binary)
        .args(["edit", source.to_str().unwrap(), operation])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    let error = String::from_utf8_lossy(&refused.stderr);
    assert!(error.contains("analysis_input_conflict"), "{error}");
    assert!(
        error.contains("preserve_copy")
            && error.contains("break_evidence")
            && error.contains("remove_analysis")
    );
    assert_eq!(std::fs::read(&source).unwrap(), original);
}

#[test]
fn species_cli_and_public_sdk_add_to_one_canonical_workspace() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let profile = parent.path().join("profile");
    let source = root.join("source.nwk");
    let canonical = root.join("species.spart");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let binary = env!("CARGO_BIN_EXE_basecraft");
    let setup = Command::new(binary)
        .args([
            "tool",
            "delimitation",
            source.to_str().unwrap(),
            "--output",
            canonical.to_str().unwrap(),
            "--params",
            r#"{"method":"workspace_tree"}"#,
        ])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .output()
        .unwrap();
    assert!(
        setup.status.success(),
        "{}",
        String::from_utf8_lossy(&setup.stderr)
    );
    let params = serde_json::json!({"method":"gmyc_single","analysis_input":source,"rooted":true})
        .to_string();
    let fitted = Command::new(binary)
        .args([
            "tool",
            "delimitation",
            canonical.to_str().unwrap(),
            "--output",
            // Relative: tools run in their own folder, so this must resolve here.
            "fit.json",
            "--params",
            &params,
        ])
        .current_dir(&root)
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .output()
        .unwrap();
    assert!(
        fitted.status.success(),
        "{}",
        String::from_utf8_lossy(&fitted.stderr)
    );
    // GMYC adds every distinct partition in its support set, best first.
    let fitted = basecraft_ops::files::load_delimitation(&canonical)
        .unwrap()
        .hypotheses;
    assert_ne!(fitted.len(), 0);
    assert!(fitted[0].name.starts_with("GMYC species hypothesis 1 ("));
    let sdk = Command::new("python3").args(["-c",r"
import json, sys
from basecraft.delimitation import run
print(json.dumps(run(sys.argv[1],sys.argv[2],method='gmyc_multiple',analysis_input=sys.argv[3],rooted=True,candidate_budget=100)))
",canonical.to_str().unwrap(),root.join("sdk.json").to_str().unwrap(),source.to_str().unwrap()]).env("BASECRAFT",binary).env("BASECRAFT_PROFILE_DIR",&profile).env("PYTHONPATH",std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src")).output().unwrap();
    assert!(
        sdk.status.success(),
        "{}",
        String::from_utf8_lossy(&sdk.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&sdk.stdout).unwrap();
    assert_eq!(result["outputs"][0], canonical.to_str().unwrap());
    let combined = basecraft_ops::files::load_delimitation(&canonical)
        .unwrap()
        .hypotheses;
    assert!(combined.len() > fitted.len());
    assert_eq!(combined[..fitted.len()], fitted[..]);
    let mut visible: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".basecraft")
        .collect();
    visible.sort();
    assert_eq!(visible, ["source.nwk", "species.spart"]);
    assert_eq!(
        basecraft_ops::provenance::verify(&canonical).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn distance_matrix_cli_keeps_only_tsv_visible() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.afa");
    let output = root.join("matrix.tsv");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n>c\nAATT\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args([
            "distance",
            source.to_str().unwrap(),
            "--matrix",
            "--output",
            output.to_str().unwrap(),
            "--model",
            "p",
        ])
        .env("BASECRAFT_PROFILE_DIR", parent.path().join("profile"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let run = basecraft_ops::analysis_store::ArtifactRun::for_result(&output)
        .unwrap()
        .unwrap();
    assert!(run.join("matrix.tsv.distances.json").is_file());
    assert!(run.join("matrix.tsv.run.json").is_file());
    assert!(!run.join("submitted.fasta").exists());
    let mut names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [".basecraft", "matrix.tsv", "source.afa"]);
    assert_eq!(
        basecraft_ops::provenance::verify(&output).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn public_matrix_cache_creation_reuse_and_failures_are_managed() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.afa");
    let cache = root.join("chosen-cache.json");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n>c\nAATT\n").unwrap();
    let run_cli = |output: &std::path::Path, chosen: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args([
                "distance",
                source.to_str().unwrap(),
                "--matrix",
                "--output",
                output.to_str().unwrap(),
                "--model",
                "p",
                "--cache",
                chosen.to_str().unwrap(),
            ])
            .env("BASECRAFT_PROFILE_DIR", parent.path().join("profile"))
            .output()
            .unwrap()
    };
    for (stem, reused) in [("created", false), ("reused", true)] {
        let before = std::fs::read(&cache).ok();
        let output = root.join(format!("{stem}.tsv"));
        let result = run_cli(&output, &cache);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let run = basecraft_ops::analysis_store::ArtifactRun::for_result(&output)
            .unwrap()
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(run.join(format!("{stem}.tsv.run.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(report["cache_reused"], reused);
        assert!(run.join(format!("{stem}.tsv.distances.json")).is_file());
        assert!(run.join("manifest.json").is_file());
        if let Some(before) = before {
            assert_eq!(std::fs::read(&cache).unwrap(), before);
        }
        assert_eq!(
            basecraft_ops::provenance::verify(&output).unwrap(),
            Vec::<String>::new()
        );
    }
    let source_before = std::fs::read(&source).unwrap();
    let alias_output = root.join("alias.tsv");
    assert!(!run_cli(&alias_output, &source).status.success());
    assert!(!alias_output.exists());
    assert_eq!(std::fs::read(&source).unwrap(), source_before);
    let same = root.join("same.tsv");
    assert!(!run_cli(&same, &same).status.success());
    assert!(!same.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let read_only = root.join("read-only");
        std::fs::create_dir(&read_only).unwrap();
        std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o555)).unwrap();
        let output = root.join("failed.tsv");
        let failed_cache = read_only.join("cache.json");
        let failure = run_cli(&output, &failed_cache);
        std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!failure.status.success());
        assert!(!output.exists());
        assert!(!failed_cache.exists());
        assert!(
            basecraft_ops::analysis_store::ArtifactRun::for_result(&output)
                .unwrap()
                .is_none()
        );
        let runs = root.join(".basecraft/analyses-v1/runs");
        assert!(
            std::fs::read_dir(runs).unwrap().any(|e| e
                .unwrap()
                .path()
                .join("pending/failed.tsv")
                .is_file())
        );
    }
    let mut files: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            e.file_type()
                .unwrap()
                .is_file()
                .then(|| e.file_name().to_string_lossy().into_owned())
        })
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "chosen-cache.json",
            "created.tsv",
            "reused.tsv",
            "source.afa"
        ]
    );
    println!("matrix --cache CLI files: {files:?}");
}
