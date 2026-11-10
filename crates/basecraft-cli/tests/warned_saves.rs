//! Real evidence ledgers and original FASTA bytes, served over the actual CLI/MCP.
use basecraft_ops::input_evidence;
use serde_json::json;
use std::{path::Path, process::Command};

#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;

#[test]
fn headless_save_conformance() {
    let Some(root) = isolated_profile::enter("headless_save_conformance") else {
        return;
    };
    let data = root.join("data");
    std::fs::create_dir(&data).unwrap();
    let original = b">gene\nACGT\n";
    let mut known = serde_json::Map::new();
    for name in [
        "known",
        "preservation",
        "writefail",
        "cli-known",
        "cli-preservation",
    ] {
        let source = data.join(format!("{name}.fasta"));
        std::fs::write(&source, original).unwrap();
        let folder = root.join(format!("analysis-{name}"));
        std::fs::create_dir(&folder).unwrap();
        let run = input_evidence::reference_input_with_dependencies(
            &folder.join("result.txt"),
            &source,
            &basecraft_io::hash_file(&source).unwrap(),
            &[],
            "adapter save regression",
        )
        .unwrap();
        known.insert(name.into(), json!(run.directory()));
    }
    let unrelated = root.join("unrelated.fasta");
    std::fs::write(&unrelated, original).unwrap();
    let damaged = input_evidence::reference_input_with_dependencies(
        &root.join("distant-result.txt"),
        &unrelated,
        &basecraft_io::hash_file(&unrelated).unwrap(),
        &[],
        "incomplete adapter evidence",
    )
    .unwrap();
    let ledger = damaged.artifact("input.json").unwrap();
    let good = std::fs::read(&ledger).unwrap();
    std::fs::write(&ledger, b"damaged ledger").unwrap();
    let config = root.join("fixture.json");
    std::fs::write(
        &config,
        json!({"known":known,"ledger":ledger,"good":String::from_utf8(good).unwrap()}).to_string(),
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/warned_save_conformance.py"))
        .arg(&data)
        .arg(&config)
        .env("BASECRAFT", env!("CARGO_BIN_EXE_basecraft"))
        .env(
            "PYTHONPATH",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
