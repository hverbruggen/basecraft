//! CLI analyses use the same registered runner, host allocation and shared
//! guarded save as the GUI.
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// Run one request with `operations run` and return its outcome.
fn run(project: &Path, profile: &Path, request: &Value) -> Value {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let index = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file = project
        .parent()
        .unwrap()
        .join(format!("request-{index}.json"));
    std::fs::write(&file, request.to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["operations", "run"])
        .arg(request["operation"].as_str().unwrap())
        .arg("--project")
        .arg(project)
        .arg("--request")
        .arg(&file)
        .env("BASECRAFT_PROFILE_DIR", profile)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn bootstrap_trees_are_retained_and_inputs_guarded() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let profile = parent.path().join("profile");
    let source = root.join("input.afa");
    std::fs::write(
        &source,
        ">a\nAAAAAACCAA\n>b\nAAAAAACCAA\n>c\nAAGGAACCAA\n>d\nAAGGAACCAA\n",
    )
    .unwrap();
    let original = std::fs::read(&source).unwrap();
    let outcome = run(
        &root,
        &profile,
        &json!({"operation":"basecraft.distance/tree@1",
            "inputs":{"alignment":[{"kind":"file","path":"input.afa"}]},
            "parameters":{"model":"p","method":"sorted","bootstrap":8,"threads":1},
            "destinations":{"artifacts/tree.nwk":"bootstrap.nwk"}}),
    );
    assert_eq!(outcome["state"], "completed", "{outcome}");
    let output = root.join("bootstrap.nwk");
    let run_id = outcome["run_id"].as_str().unwrap();
    // Replicate trees are a retained member of the run, never a visible file.
    let replicates = format!("bootstrap-{run_id}.nex");
    assert!(
        outcome["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["path"].as_str().unwrap().ends_with(&replicates)),
        "{outcome}"
    );
    let mut visible: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    visible.sort();
    assert_eq!(visible, [".basecraft", "bootstrap.nwk", "input.afa"]);
    assert!(
        basecraft_ops::analysis_store::ArtifactRun::for_result(&output)
            .unwrap()
            .is_some()
    );
    let reader = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/verify_provenance.py");
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
    let verified = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["provenance", "verify", output.to_str().unwrap()])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    // Editing an analysed input asks for a decision instead of breaking evidence.
    let operation = r#"{"op":"aln.replace","params":{"row":0,"start":0,"end":1,"residues":"T"}}"#;
    let refused = Command::new(env!("CARGO_BIN_EXE_basecraft"))
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
    let setup = run(
        &root,
        &profile,
        &json!({"operation":"basecraft.delimitation/workspace_tree@1",
            "inputs":{"tree":[{"kind":"file","path":"source.nwk"}]},
            "parameters":{},"destinations":{"workspace":"species.spart"}}),
    );
    assert_eq!(setup["state"], "completed", "{setup}");
    assert!(canonical.is_file());
    let fitted = run(
        &root,
        &profile,
        &json!({"operation":"basecraft.delimitation/gmyc_single@1",
            "inputs":{"tree":[{"kind":"file","path":"source.nwk"}]},
            "parameters":{"rooted":true},
            "target":{"kind":"file","path":"species.spart"}}),
    );
    assert_eq!(fitted["state"], "completed", "{fitted}");
    // GMYC adds every distinct partition in its support set, best first.
    let fitted = basecraft_ops::files::load_delimitation(&canonical)
        .unwrap()
        .hypotheses;
    assert_ne!(fitted.len(), 0);
    assert!(fitted[0].name.starts_with("GMYC species hypothesis 1 ("));
    let sdk = Command::new("python3")
        .args([
            "-c",
            r"
import json, sys
from basecraft.delimitation import run
print(json.dumps(run(sys.argv[1], method='gmyc_multiple', workspace=sys.argv[2], rooted=True, candidate_budget=100)))
",
            source.to_str().unwrap(),
            canonical.to_str().unwrap(),
        ])
        .env("BASECRAFT", env!("CARGO_BIN_EXE_basecraft"))
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .env(
            "PYTHONPATH",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src"),
        )
        .output()
        .unwrap();
    assert!(
        sdk.status.success(),
        "{}",
        String::from_utf8_lossy(&sdk.stderr)
    );
    let result: Value = serde_json::from_slice(&sdk.stdout).unwrap();
    assert_eq!(result["state"], "completed", "{result}");
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
fn distance_matrix_keeps_only_the_table_visible() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source.afa"), ">a\nAAAA\n>b\nAAAT\n>c\nAATT\n").unwrap();
    let outcome = run(
        &root,
        &parent.path().join("profile"),
        &json!({"operation":"basecraft.distance/matrix@1",
            "inputs":{"alignment":[{"kind":"file","path":"source.afa"}]},
            "parameters":{"model":"p"},
            "destinations":{"artifacts/distances.tsv":"matrix.tsv"}}),
    );
    assert_eq!(outcome["state"], "completed", "{outcome}");
    let output = root.join("matrix.tsv");
    // The distance cache and run report stay with the run's retained results.
    for member in ["distances.tsv.distances.json", "distances.tsv.run.json"] {
        assert!(
            outcome["artifacts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["path"].as_str().unwrap().ends_with(member)),
            "{member}: {outcome}"
        );
    }
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
