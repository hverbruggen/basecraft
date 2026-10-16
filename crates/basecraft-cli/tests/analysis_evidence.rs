//! CLI routes use the same host allocation and shared guarded save as the GUI.
use serde_json::Value;
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
        let result: Value = serde_json::from_slice(&result.stdout).unwrap();
        let collection = result["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .find(|p| p.ends_with(".nex"))
            .unwrap();
        let collection = std::path::Path::new(collection);
        assert!(collection.is_file());
        if destination == "metadata" {
            assert!(collection.starts_with(root.path().join(".basecraft/analyses-v1/runs")));
        } else {
            assert_eq!(collection.parent(), Some(root.path()));
        }
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
