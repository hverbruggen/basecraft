//! `basecraft run`: one tool on files (D1). Input files in, result files out,
//! the same run as a hand-written request, and refusals that say what to give.
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

const ALIGNMENT: &str = ">a\nACGTACGTACGTACGT\n>b\nACGTACGAACGTACGT\n>c\nTCGTACGAACGTTCGT\n>d\nTCGTACGATCGTTCGA\n>e\nTCGAACGATCGTTCGA\n";

fn basecraft(profile: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .env("BASECRAFT_PROFILE_DIR", profile)
        .output()
        .unwrap()
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn a_tool_runs_on_files_like_its_request_and_refuses_with_what_to_give() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("input.afa"), ALIGNMENT).unwrap();

    // In a project (the global --project, before the command): the alignment
    // in, the tree out, by the operation's short name.
    let ran = basecraft(
        &profile,
        &[
            "--project",
            text(&project),
            "run",
            "distance/tree",
            text(&project.join("input.afa")),
            "-o",
            text(&project.join("run.nwk")),
            "--param",
            "model=p",
        ],
    );
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let outcome: Value = serde_json::from_slice(&ran.stdout).unwrap();
    assert_eq!(outcome["state"], "completed", "{outcome}");

    // The same run as a hand-written request gives the same tree.
    let request = temp.path().join("request.json");
    std::fs::write(
        &request,
        json!({"operation": "basecraft.distance/tree@1",
            "inputs": {"alignment": [{"kind": "file", "path": "input.afa"}]},
            "parameters": {"model": "p"},
            "destinations": {"artifacts/tree.nwk": "request.nwk"}})
        .to_string(),
    )
    .unwrap();
    let requested = basecraft(
        &profile,
        &[
            "operations",
            "run",
            "basecraft.distance/tree@1",
            "--project",
            text(&project),
            "--request",
            text(&request),
        ],
    );
    assert!(
        requested.status.success(),
        "{}",
        String::from_utf8_lossy(&requested.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(project.join("run.nwk")).unwrap(),
        std::fs::read_to_string(project.join("request.nwk")).unwrap()
    );

    // A loose file: no project, and the run's records go beside it.
    let loose = temp.path().join("loose");
    std::fs::create_dir(&loose).unwrap();
    std::fs::write(loose.join("input.afa"), ALIGNMENT).unwrap();
    let ran = basecraft(
        &profile,
        &[
            "run",
            "basecraft.distance/tree@1",
            text(&loose.join("input.afa")),
            "-o",
            text(&loose.join("tree.nwk")),
            "--param",
            "model=p",
        ],
    );
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(loose.join("tree.nwk").is_file());
    assert!(loose.join(".basecraft").is_dir());

    // Refusals say what to give.
    let unknown = basecraft(
        &profile,
        &["run", "no-such-tool", text(&project.join("input.afa"))],
    );
    assert!(!unknown.status.success());
    assert!(
        String::from_utf8_lossy(&unknown.stderr)
            .contains("no installed operation is called no-such-tool")
    );
    let unpaired = basecraft(
        &profile,
        &[
            "run",
            "distance/tree",
            "--input",
            text(&project.join("input.afa")),
        ],
    );
    assert!(!unpaired.status.success());
    // A setting the tool does not have is named, with the ones it has.
    let unknown_setting = basecraft(
        &profile,
        &[
            "run",
            "distance/tree",
            text(&project.join("input.afa")),
            "-o",
            text(&project.join("other.nwk")),
            "--param",
            "colour=red",
            "--project",
            text(&project),
        ],
    );
    let said = String::from_utf8_lossy(&unknown_setting.stderr);
    assert!(!unknown_setting.status.success());
    assert!(
        said.contains("no setting colour") && said.contains("model"),
        "{said}"
    );
    assert!(String::from_utf8_lossy(&unpaired.stderr).contains("expected --input ROLE=FILE"));
}
