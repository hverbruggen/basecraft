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
            root.join("fit.json").to_str().unwrap(),
            "--params",
            &params,
        ])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .output()
        .unwrap();
    assert!(
        fitted.status.success(),
        "{}",
        String::from_utf8_lossy(&fitted.stderr)
    );
    assert_eq!(
        basecraft_ops::files::load_delimitation(&canonical)
            .unwrap()
            .hypotheses
            .len(),
        1
    );
    let sdk = Command::new("python3").args(["-c",r#"
import json, sys
from basecraft.delimitation import run
print(json.dumps(run(sys.argv[1],sys.argv[2],method='gmyc_multiple',analysis_input=sys.argv[3],rooted=True,candidate_budget=100)))
"#,canonical.to_str().unwrap(),root.join("sdk.json").to_str().unwrap(),source.to_str().unwrap()]).env("BASECRAFT",binary).env("BASECRAFT_PROFILE_DIR",&profile).env("PYTHONPATH",std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src")).output().unwrap();
    assert!(
        sdk.status.success(),
        "{}",
        String::from_utf8_lossy(&sdk.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&sdk.stdout).unwrap();
    assert_eq!(result["outputs"][0], canonical.to_str().unwrap());
    assert_eq!(
        basecraft_ops::files::load_delimitation(&canonical)
            .unwrap()
            .hypotheses
            .len(),
        2
    );
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
