//! Attach a display tree to a species workspace and write its batch figures
//! from the command line (D1), through the same calls as the window.
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

    // Batch figures: every hypothesis, then one per hypothesis.
    let figures = folder.join("figures");
    let written = basecraft(
        &profile,
        &[
            "delimitation",
            "figures",
            &path(&workspace),
            "-o",
            &path(&figures),
        ],
    );
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&written.stdout).unwrap();
    let files = value["figures"].as_array().unwrap();
    assert_eq!(
        files.len(),
        3,
        "all, then each of the two hypotheses: {value}"
    );
    assert!(figures.join("hypotheses.svg").is_file());
    for file in files {
        let svg = std::fs::read_to_string(file.as_str().unwrap()).unwrap();
        assert!(svg.contains("<svg"), "{file}");
    }
}
