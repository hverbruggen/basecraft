//! Sanger review decisions as commands (D1): start a review from a read,
//! decide its columns, export the consensus, and refuse with what to give.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn basecraft(profile: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .env("BASECRAFT_PROFILE_DIR", profile)
        .output()
        .unwrap()
}

fn json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn a_read_is_reviewed_decided_and_exported_from_the_command_line() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let folder = temp.path().join("reads");
    std::fs::create_dir(&folder).unwrap();
    let read = folder.join("forward.ab1");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &read,
    )
    .unwrap();
    let review = folder.join("forward.sanger-review.json");
    let path = |p: &Path| p.to_str().unwrap().to_owned();

    let started = json(&basecraft(
        &profile,
        &["sanger", "review", &path(&read), "-o", &path(&review)],
    ));
    let columns = started["columns"].as_u64().unwrap();
    assert!(columns > 0 && review.is_file(), "{started}");

    // The consensus waits for every column.
    let consensus = folder.join("consensus.fasta");
    let early = basecraft(
        &profile,
        &[
            "sanger",
            "export",
            &path(&review),
            "-o",
            &path(&consensus),
            "--label",
            "HV1",
        ],
    );
    assert!(!early.status.success());
    assert!(String::from_utf8_lossy(&early.stderr).contains("Resolve every review column"));

    let all = format!("1-{columns}");
    let decided = json(&basecraft(
        &profile,
        &[
            "sanger",
            "decide",
            &path(&review),
            "--columns",
            &all,
            "--choice",
            "accept",
        ],
    ));
    assert_eq!(decided["unresolved"], 0, "{decided}");
    json(&basecraft(
        &profile,
        &[
            "sanger",
            "decide",
            &path(&review),
            "--columns",
            "1",
            "--choice",
            "a",
        ],
    ));
    json(&basecraft(
        &profile,
        &[
            "sanger",
            "export",
            &path(&review),
            "-o",
            &path(&consensus),
            "--label",
            "HV1",
        ],
    ));
    assert!(
        std::fs::read_to_string(&consensus)
            .unwrap()
            .starts_with(">HV1")
    );

    // A decision that is not one is refused, saying what to give.
    let refused = basecraft(
        &profile,
        &[
            "sanger",
            "decide",
            &path(&review),
            "--columns",
            "1",
            "--choice",
            "maybe",
        ],
    );
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("give accept, reject, pending or a base")
    );
}
