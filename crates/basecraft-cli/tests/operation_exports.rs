//! A destination `role/member` exports one validated collection member as a file.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::links::AlignmentLinkExt;
use std::process::Command;

use serde_json::{Value, json};

#[test]
fn member_destinations_export_one_file_and_never_replace() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        project.join("input.afa"),
        ">a\nACGTACGTACGTACGT\n>b\nACGTACGAACGTACGT\n>c\nTCGTACGAACGTTCGT\n>d\nTCGTACGATCGTTCGA\n>e\nTCGAACGATCGTTCGA\n",
    )
    .unwrap();
    let run = |destinations: Value, name: &str| {
        let parameters = if name == "bootstrap" {
            json!({"model":"p","bootstrap":10,"seed":3})
        } else {
            json!({"model":"p"})
        };
        let request = temp.path().join(format!("{name}.json"));
        std::fs::write(
            &request,
            json!({"operation":"basecraft.distance/tree@1",
                "inputs":{"alignment":[{"kind":"file","path":"input.afa"}]},
                "parameters":parameters,"destinations":destinations})
            .to_string(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args([
                "operations",
                "run",
                "basecraft.distance/tree@1",
                "--project",
            ])
            .arg(&project)
            .arg("--request")
            .arg(&request)
            .env("BASECRAFT_PROFILE_DIR", &profile)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let outcome = run(json!({"artifacts/tree.nwk":"tree.nwk"}), "first");
    assert_eq!(outcome["state"], "completed", "{outcome}");
    let exported = project.join("tree.nwk");
    let digest = basecraft_io::hash_file(&exported).unwrap();
    assert!(
        outcome["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["path"] == exported.display().to_string() && a["digest"] == digest),
        "{outcome}"
    );
    // The retained member and the exported file are the same bytes.
    assert!(
        outcome["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["path"].as_str().unwrap().ends_with("tree.nwk"))
            .all(|a| a["digest"] == digest)
    );
    // The exported tree is linked to its alignment and bound to the run.
    let link = basecraft_ops::links::AlignmentLink::read(&exported).unwrap();
    assert_eq!(
        link.source.canonicalize().unwrap(),
        project.join("input.afa").canonicalize().unwrap()
    );
    // Producer support semantics travel with an exported tree.
    let bootstrap = run(json!({"artifacts/tree.nwk":"bootstrap.nwk"}), "bootstrap");
    assert_eq!(bootstrap["state"], "completed", "{bootstrap}");
    let tree = basecraft_ops::files::load_tree(&project.join("bootstrap.nwk"))
        .unwrap()
        .doc;
    assert_eq!(
        tree.supports()
            .iter()
            .map(|support| support.name.as_str())
            .collect::<Vec<_>>(),
        ["Bootstrap"]
    );
    // An existing destination is never replaced.
    std::fs::write(project.join("taken.nwk"), "foreign").unwrap();
    let conflict = run(json!({"artifacts/tree.nwk":"taken.nwk"}), "conflict");
    assert_ne!(conflict["state"], "completed", "{conflict}");
    assert_eq!(
        std::fs::read_to_string(project.join("taken.nwk")).unwrap(),
        "foreign"
    );
    // Members must be plain relative names of the role.
    let escape = run(json!({"artifacts/../x.nwk":"x.nwk"}), "escape");
    assert_eq!(escape["errors"][0]["code"], "invalid_arguments", "{escape}");
}
