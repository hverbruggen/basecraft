//! `--project` decides where a file's history goes; without it files are loose.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::process::Command;

fn edit(args: &[&str], file: &std::path::Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .args([
            "edit",
            file.to_str().unwrap(),
            r#"{"op":"seq.replace","params":{"start":0,"end":1,"residues":"T"}}"#,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn history_goes_to_the_chosen_project_or_beside_a_loose_file() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let chosen = root.path().join("project");
    std::fs::create_dir(&chosen).unwrap();
    let inside = chosen.join("inside.fasta");
    std::fs::write(&inside, b">a\nACGT\n").unwrap();
    edit(&["--project", chosen.to_str().unwrap()], &inside);
    let project = basecraft_ops::project::Project::new(&chosen).unwrap();
    let bundle = project.provenance_bundle(&inside).unwrap();
    assert!(bundle.join("manifest.json").is_file());
    assert!(!basecraft_ops::records::companion(&inside).exists());

    let loose = root.path().join("loose.fasta");
    std::fs::write(&loose, b">a\nACGT\n").unwrap();
    edit(&[], &loose);
    assert!(
        basecraft_ops::records::companion(&loose)
            .join("manifest.json")
            .is_file()
    );
}
