use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn request(id: u32, name: &str, args: Value) -> Value {
    let mut message = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name}});
    message["params"]["arguments"] = args;
    message
}

#[test]
fn stdio_edits_are_version_checked_logged_and_project_scoped() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("input.afa"), ">a\nACGT\n>b\nACGT\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args([
            "mcp",
            d.path().to_str().unwrap(),
            "--allow-writes",
            "--model-id",
            "test-agent",
        ])
        .env("BASECRAFT_PROFILE_DIR", d.path().join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let messages = vec![
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        request(1, "open", json!({"path":"input.afa"})),
        request(
            2,
            "apply",
            json!({"document":0,"version":0,"operation":{"op":"aln.replace","params":{"row":0,"start":0,"end":1,"residues":"T"}}}),
        ),
        request(
            3,
            "apply",
            json!({"document":0,"version":0,"operation":{"op":"aln.remove_gap_columns","params":{}}}),
        ),
        request(4, "read", json!({"document":0,"row":0,"start":0,"end":4})),
        request(5, "save", json!({"document":0})),
        request(6, "open", json!({"path":"/etc/passwd"})),
        request(7, "state", json!({"document":999})),
    ];
    let mut input = child.stdin.take().unwrap();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(replies.len(), 8);
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(replies[2]["result"]["isError"], false);
    assert_eq!(replies[3]["result"]["isError"], true);
    assert!(
        replies[4]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("TCGT")
    );
    assert_eq!(replies[6]["result"]["isError"], true);
    assert_eq!(replies[7]["result"]["isError"], true);
    assert!(
        std::fs::read_to_string(d.path().join("input.afa"))
            .unwrap()
            .contains("TCGT")
    );
    let log = basecraft_ops::provenance::inspect(&d.path().join("input.afa"))
        .unwrap()
        .events;
    assert!(log.iter().any(
        |e| matches!(&e.actor,basecraft_protocol::Actor::Agent{model} if model=="test-agent")
    ));
    assert_eq!(
        basecraft_ops::provenance::verify(&d.path().join("input.afa")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn mcp_defaults_to_read_only() {
    let d = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["mcp", d.path().to_str().unwrap()])
        .env("BASECRAFT_PROFILE_DIR", d.path().join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":0,"method":"initialize"})
    )
    .unwrap();
    writeln!(input, "{}", request(1, "apply", json!({}))).unwrap();
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().contains("read-only"));
}

#[test]
fn document_record_describes_unsaved_headless_edits_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("gene.fasta"), ">gene\nACGT\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["mcp", dir.path().to_str().unwrap(), "--allow-writes"])
        .env("BASECRAFT_PROFILE_DIR", dir.path().join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize"}),
        request(1, "open", json!({"path":"gene.fasta"})),
        request(
            2,
            "apply",
            json!({"document":0,"version":0,"operation":{"op":"seq.replace","params":{"start":0,"end":1,"residues":"T"}}}),
        ),
        request(3, "document_record", json!({"document":0})),
        request(4, "undo", json!({"document":0})),
        request(5, "document_record", json!({"document":0})),
    ] {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    let replies: Vec<Value> = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let before: Value =
        serde_json::from_str(replies[3]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(before["unsaved"], true);
    assert!(before["methods"].as_str().unwrap().contains("Replaced"));
    let after: Value =
        serde_json::from_str(replies[5]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(after["unsaved"], false);
    assert_eq!(after["paragraphs"], json!([]));
    assert!(!basecraft_ops::provenance::companion(&dir.path().join("gene.fasta")).exists());
}

#[test]
fn species_mcp_uses_the_canonical_workspace_contract() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("source.nwk"),
        "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n",
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["mcp", root.to_str().unwrap(), "--allow-writes"])
        .env("BASECRAFT_PROFILE_DIR", parent.path().join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize"}),
        request(1, "open", json!({"path":"source.nwk"})),
        request(
            2,
            "run_tool",
            json!({"document":0,"tool":"delimitation","output":"species.spart","params":{"method":"workspace_tree"}}),
        ),
        request(3, "open", json!({"path":"species.spart"})),
        request(
            4,
            "run_tool",
            json!({"document":1,"tool":"delimitation","output":"fit.json","params":{"method":"gmyc_single","rooted":true,"analysis_input":"source.nwk"}}),
        ),
    ];
    let mut input = child.stdin.take().unwrap();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for reply in &replies[1..] {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
    }
    let manifest: Value =
        serde_json::from_str(replies[4]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        manifest["outputs"][0],
        root.join("species.spart").to_str().unwrap()
    );
    assert!(!root.join("fit.json").exists());
    // GMYC publishes each distinct partition in its support set, best first.
    let hypotheses = basecraft_ops::files::load_delimitation(&root.join("species.spart"))
        .unwrap()
        .hypotheses;
    assert_ne!(hypotheses.len(), 0);
    assert!(
        hypotheses
            .iter()
            .all(|h| h.name.starts_with("GMYC species hypothesis "))
    );
    assert_eq!(
        basecraft_ops::provenance::verify(&root.join("species.spart")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn tree_mcp_routes_share_hidden_storage() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source.afa"), ">a\nACGTACGTACGTACGTACGTACGT\n>b\nACGTACGTTCGTACGTACGTACGT\n>c\nTCGTACGTACGTACGTACGTACGA\n>d\nTCGTACGTTCGTACGTACGTACGA\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["mcp", root.to_str().unwrap(), "--allow-writes"])
        .env("BASECRAFT_PROFILE_DIR", parent.path().join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize"}),
        request(1, "open", json!({"path":"source.afa"})),
        request(
            2,
            "run_tool",
            json!({"document":0,"tool":"distance","output":"nj.nwk","params":{"method":"sorted","model":"p","threads":1}}),
        ),
        request(
            3,
            "run_tool",
            json!({"document":0,"tool":"distance","output":"upgma.nwk","params":{"method":"upgma","model":"p","threads":1}}),
        ),
        request(
            4,
            "run_tool",
            json!({"document":0,"tool":"fasttree","output":"fasttree.nwk","params":{"threads":1}}),
        ),
        request(
            5,
            "run_tool",
            json!({"document":0,"tool":"iqtree","output":"iqtree.nwk","params":{"model":"JC","threads":1,"seed":71}}),
        ),
    ];
    let mut input = child.stdin.take().unwrap();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    let replies: Vec<Value> = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 6);
    for reply in &replies[1..] {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
    }
    let mut visible: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    visible.sort();
    assert_eq!(
        visible,
        [
            ".basecraft",
            "fasttree.nwk",
            "iqtree.nwk",
            "nj.nwk",
            "source.afa",
            "upgma.nwk"
        ]
    );
    println!("MCP trees: {visible:?}");
    for name in ["fasttree.nwk", "iqtree.nwk", "nj.nwk", "upgma.nwk"] {
        let output = root.join(name);
        let run = basecraft_ops::analysis_store::ArtifactRun::for_result(&output)
            .unwrap()
            .unwrap();
        assert!(!run.join("submitted.fasta").exists());
        assert!(run.join("execution.json").is_file());
        assert_eq!(
            basecraft_ops::provenance::verify(&output).unwrap(),
            Vec::<String>::new()
        );
    }
}
