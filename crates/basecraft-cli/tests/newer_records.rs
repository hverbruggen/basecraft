//! The CLI refuses a save that would drop fields a later basecraft wrote,
//! naming both versions, and saves when told to drop them (D14).
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn edit(root: &Path, tip: &str, extra: &[&str]) -> std::process::Output {
    let op = json!({"op": "tree.reroot", "params": {"clade": [tip]}}).to_string();
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(extra)
        .arg("edit")
        .arg(root.join("t.nwk"))
        .arg(op)
        .env("BASECRAFT_PROFILE_DIR", root.join("profile"))
        .output()
        .unwrap()
}

fn side_file(root: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(root.join("t.nwk.basecraft.json")).unwrap()).unwrap()
}

#[test]
fn cli_refuses_to_drop_newer_fields_unless_told_to() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("t.nwk"), "[&R] ((a,b),(c,d));\n").unwrap();
    assert!(edit(root, "b", &[]).status.success());
    let mut record = side_file(root);
    assert_eq!(record["basecraft"], basecraft_protocol::VERSION);
    record["basecraft"] = "99.0.0".into();
    record["future"] = json!({"kept": 1});
    std::fs::write(root.join("t.nwk.basecraft.json"), record.to_string()).unwrap();

    let refused = edit(root, "c", &[]);
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    for part in [
        "newer_fields_dropped",
        "99.0.0",
        basecraft_protocol::VERSION,
        "future",
    ] {
        assert!(stderr.contains(part), "{stderr}");
    }
    assert_eq!(side_file(root)["future"], json!({"kept": 1}));

    let saved = edit(root, "c", &["--drop-newer-fields"]);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    let record = side_file(root);
    assert_eq!(record["basecraft"], basecraft_protocol::VERSION);
    assert!(record.get("future").is_none());
}
