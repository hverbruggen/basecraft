//! The CLI refuses a save that would drop fields a later basecraft wrote,
//! naming both versions, and saves when told to drop them (D14).
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

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
    serde_json::from_slice(
        &std::fs::read(basecraft_ops::files::companion_path(
            None,
            &root.join("t.nwk"),
        ))
        .unwrap(),
    )
    .unwrap()
}

/// A tree whose side file a later basecraft wrote, with a field this
/// build does not know.
fn newer_side_file(root: &Path) {
    std::fs::write(root.join("t.nwk"), "[&R] ((a,b),(c,d));\n").unwrap();
    assert!(edit(root, "b", &[]).status.success());
    let mut record = side_file(root);
    assert_eq!(record["basecraft"], basecraft_protocol::VERSION);
    record["basecraft"] = "99.0.0".into();
    record["future"] = json!({"kept": 1});
    std::fs::write(
        basecraft_ops::files::companion_path(None, &root.join("t.nwk")),
        record.to_string(),
    )
    .unwrap();
}

#[test]
fn cli_refuses_to_drop_newer_fields_unless_told_to() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    newer_side_file(root);

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

#[test]
fn mcp_refuses_to_drop_newer_fields_unless_told_to() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    newer_side_file(root);
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["mcp", root.to_str().unwrap(), "--allow-writes"])
        .env("BASECRAFT_PROFILE_DIR", root.join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let call = |id: u32, name: &str, args: Value| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
    let reroot = json!({"op": "tree.reroot", "params": {"clade": ["c"]}});
    let messages = [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        call(1, "open", json!({"path": "t.nwk"})),
        call(
            2,
            "apply",
            json!({"document": 0, "version": 0, "operation": reroot}),
        ),
        call(3, "save", json!({"document": 0})),
        call(4, "save", json!({"document": 0, "drop_newer_fields": true})),
    ];
    let mut input = child.stdin.take().unwrap();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let out = child.wait_with_output().unwrap();
    let replies: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    // The notification gets no reply: replies are initialize, open, apply,
    // then the two saves.
    let refused = &replies[3]["result"];
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["errors"][0]["code"],
        "newer_fields_dropped"
    );
    let message = refused["structuredContent"]["errors"][0]["message"].to_string();
    assert!(
        message.contains("99.0.0") && message.contains("future"),
        "{message}"
    );
    assert_eq!(replies[4]["result"]["isError"], false, "{}", replies[4]);
    assert!(side_file(root).get("future").is_none());
}
