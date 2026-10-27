//! A destination `role/member` exports one validated collection member as a file.
use std::process::Command;

use serde_json::{Value, json};

#[test]
fn member_destinations_export_one_file_and_never_replace() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        project.join("input.afa"),
        ">a\nACGTACGT\n>b\nACGTACGA\n>c\nTCGTACGA\n",
    )
    .unwrap();
    let run = |destinations: Value, name: &str| {
        let request = temp.path().join(format!("{name}.json"));
        std::fs::write(
            &request,
            json!({"operation":"basecraft.distance/tree@1",
                "inputs":{"alignment":[{"kind":"file","path":"input.afa"}]},
                "parameters":{"model":"p"},"destinations":destinations})
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
