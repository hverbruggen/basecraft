use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .current_dir(root)
        .env("BASECRAFT_PROFILE_DIR", root.join("profile"))
        .env("BASECRAFT_PLUGINS", root.join("no-bundle"))
        .env("BASECRAFT_FIXTURE_SENTINEL", root.join("sentinel"))
        .output()
        .unwrap()
}
fn value(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn external_metadata_install_warning_and_disable_policy_from_outside_repository() {
    let temp = tempfile::tempdir().unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin");
    let fixture = fixture.to_str().unwrap();
    let listed = value(&run(temp.path(), &["plugins", "list", "--json"]));
    assert_eq!(listed["packages"], serde_json::json!([]));
    assert!(!temp.path().join("profile").exists());
    value(&run(temp.path(), &["plugins", "validate", fixture]));
    assert!(!temp.path().join("sentinel").exists());
    assert!(!temp.path().join("profile").exists());
    let installed = run(temp.path(), &["plugins", "install", fixture]);
    assert_eq!(
        value(&installed)["installed"],
        "org.example.registry-fixture"
    );
    let warning = String::from_utf8(installed.stderr).unwrap();
    assert!(warning.contains("user account's permissions"));
    assert!(warning.contains("responsible for checking"));
    let listed = run(temp.path(), &["operations", "list", "--json"]);
    let metadata = value(&listed);
    assert_eq!(listed.stderr, Vec::<u8>::new());
    assert_eq!(metadata["operations"][0]["enabled"], true);
    assert_eq!(metadata["operations"][0]["runtime"], "unverified");
    let id = "org.example.registry-fixture/count@1";
    let detail = value(&run(temp.path(), &["operations", "describe", id, "--json"]));
    assert_eq!(detail, metadata["operations"][0]);
    assert_eq!(
        detail["descriptor"]["parameters"]["additionalProperties"],
        false
    );
    value(&run(
        temp.path(),
        &["plugins", "disable", "org.example.registry-fixture"],
    ));
    value(&run(temp.path(), &["plugins", "install", fixture]));
    let detail = value(&run(temp.path(), &["operations", "describe", id, "--json"]));
    assert_eq!(detail["enabled"], false);
    value(&run(
        temp.path(),
        &["plugins", "enable", "org.example.registry-fixture"],
    ));
    value(&run(
        temp.path(),
        &["plugins", "remove", "org.example.registry-fixture"],
    ));
    assert_eq!(
        value(&run(temp.path(), &["operations", "list"]))["operations"],
        serde_json::json!([])
    );
    assert!(!temp.path().join("sentinel").exists());
}
