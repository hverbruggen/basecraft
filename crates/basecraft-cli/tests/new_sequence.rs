#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn new_sequence_cli_refuses_overwrite_and_mcp_uses_shared_creation() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("cli.gb");
    let args = [
        "new-sequence",
        "--name",
        "sample",
        "--sequence",
        "ac gt",
        "--output",
        output.to_str().unwrap(),
    ];
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args(args)
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .output()
            .unwrap()
    };
    assert!(invoke().status.success());
    let original = std::fs::read(&output).unwrap();
    assert!(!invoke().status.success());
    assert_eq!(std::fs::read(&output).unwrap(), original);
    for writes in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_basecraft"));
        command.args(["mcp", root.path().to_str().unwrap()]);
        if writes {
            command.arg("--allow-writes");
        }
        let mut child = command
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let call = |id, name, args| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
        let messages = [
            json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
            call(
                1,
                "sequence_create",
                json!({"name":"mcp_sample","sequence":"acgt"}),
            ),
            call(2, "save", json!({"document":0,"path":"mcp.gb"})),
        ];
        let mut input = child.stdin.take().unwrap();
        for message in messages {
            writeln!(input, "{message}").unwrap();
        }
        drop(input);
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        let replies: Vec<Value> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(replies[1]["result"]["isError"], !writes, "{replies:?}");
        assert_eq!(replies[2]["result"]["isError"], !writes, "{replies:?}");
    }
    let record = std::fs::read_to_string(root.path().join("mcp.gb")).unwrap();
    assert!(record.contains("mcp_sample"));
}

/// D21: a new document needs a target. The CLI refuses one without --output
/// and the MCP server without a project folder; neither writes anything.
#[test]
fn new_documents_need_a_target_path_or_project() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args(args)
            .current_dir(root.path())
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let cli = run(&["new-sequence", "--name", "sample", "--sequence", "acgt"]);
    assert!(!cli.status.success());
    assert!(String::from_utf8_lossy(&cli.stderr).contains("--output"));
    let mcp = run(&["mcp"]);
    assert!(!mcp.status.success());
    assert!(String::from_utf8_lossy(&mcp.stderr).contains("<PROJECT>"));
    let written: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name != "profile")
        .collect();
    assert!(written.is_empty(), "{written:?}");
}
