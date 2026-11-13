//! Attach a display tree to a species workspace from the command line (D1),
//! through the same call as the window.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::path::Path;
use std::process::{Command, Output};

fn basecraft(profile: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .env("BASECRAFT_PROFILE_DIR", profile)
        .output()
        .unwrap()
}

#[test]
fn a_display_tree_is_attached_and_saved_from_the_command_line() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let folder = temp.path().join("species");
    std::fs::create_dir(&folder).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/compact/species-delimitation");
    let workspace = folder.join("hypotheses.spart");
    let tree = folder.join("time.nwk");
    std::fs::copy(fixtures.join("hypotheses.spart"), &workspace).unwrap();
    std::fs::copy(fixtures.join("time.nwk"), &tree).unwrap();
    let two = folder.join("two.nwk");
    std::fs::write(&two, "(a,b);\n(a,b);\n").unwrap();
    let path = |p: &Path| p.to_str().unwrap().to_owned();
    let attach = |tree: &Path| {
        basecraft(
            &profile,
            &[
                "delimitation",
                "display-tree",
                &path(&workspace),
                "--tree",
                &path(tree),
            ],
        )
    };

    let refused = attach(&two);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("exactly one display tree"));

    let attached = attach(&tree);
    assert!(
        attached.status.success(),
        "{}",
        String::from_utf8_lossy(&attached.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&attached.stdout).unwrap();
    assert_eq!(value["display_tree"], path(&tree.canonicalize().unwrap()));
    // Saved: attaching it again from a new session finds it already there.
    let again: serde_json::Value = serde_json::from_slice(&attach(&tree).stdout).unwrap();
    assert_eq!(again["display_tree"], value["display_tree"]);
}
