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
    assert_eq!(
        replies[3]["result"]["structuredContent"]["errors"][0]["code"],
        "stale_revision"
    );
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
fn script_clients_record_script_edits() {
    use basecraft_protocol::Actor;
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("input.afa");
    std::fs::write(&file, ">a\nACGT\n>b\nACGT\n").unwrap();
    let profile = d.path().join("profile");
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args([
            "mcp",
            d.path().to_str().unwrap(),
            "--allow-writes",
            "--client-kind",
            "script",
            "--client",
            "Python",
        ])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        request(1, "open", json!({"path":"input.afa"})),
        request(
            2,
            "apply",
            json!({"document":0,"version":0,"operation":{"op":"aln.replace","params":{"row":0,"start":0,"end":1,"residues":"T"}}}),
        ),
        request(3, "save", json!({"document":0})),
    ] {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    for line in String::from_utf8(out.stdout).unwrap().lines() {
        let reply: Value = serde_json::from_str(line).unwrap();
        assert!(reply["result"]["isError"] != true, "{reply}");
    }
    // A plain CLI edit is a script edit too, by the CLI client.
    let edited = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args([
            "edit",
            file.to_str().unwrap(),
            r#"{"op":"aln.replace","params":{"row":1,"start":0,"end":1,"residues":"G"}}"#,
        ])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .output()
        .unwrap();
    assert!(
        edited.status.success(),
        "{}",
        String::from_utf8_lossy(&edited.stderr)
    );
    let events = basecraft_ops::provenance::inspect(&file).unwrap().events;
    let actors: Vec<&Actor> = events
        .iter()
        .filter(|e| matches!(e.op, basecraft_protocol::Op::AlnReplace { .. }))
        .map(|e| &e.actor)
        .collect();
    assert_eq!(
        actors,
        [
            &Actor::Script {
                client: "Python".into()
            },
            &Actor::Script {
                client: "CLI".into()
            }
        ]
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e.actor, Actor::Agent { .. }))
    );
    assert_eq!(
        basecraft_ops::provenance::verify(&file).unwrap(),
        Vec::<String>::new()
    );
    // Read back through the history command: script edits show as script edits.
    let history = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["history", file.to_str().unwrap(), "--json"])
        .env("BASECRAFT_PROFILE_DIR", &profile)
        .output()
        .unwrap();
    assert!(history.status.success());
    let history: Value = serde_json::from_slice(&history.stdout).unwrap();
    let details: Vec<&str> = history["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["details"].as_str())
        .collect();
    assert!(
        details.iter().any(|d| d.starts_with("Script: Python")),
        "{details:#?}"
    );
    assert!(
        details.iter().any(|d| d.starts_with("Script: CLI")),
        "{details:#?}"
    );
    assert!(
        !details.iter().any(|d| d.starts_with("Agent:")),
        "{details:#?}"
    );
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("TCGT\n>b\nGCGT")
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
    writeln!(
        input,
        "{}",
        request(2, "basecraft.host.jobs_get.v1", json!({}))
    )
    .unwrap();
    writeln!(
        input,
        "{}",
        request(3, "basecraft.host.operations_list.v1", json!({}))
    )
    .unwrap();
    writeln!(
        input,
        "{}",
        request(
            4,
            "basecraft.host.operations_describe.v1",
            json!({"operation":"basecraft.distance/tree@1"})
        )
    )
    .unwrap();
    writeln!(
        input,
        "{}",
        request(
            5,
            "basecraft.host.operations_describe.v1",
            json!({"operation":"basecraft.distance/missing@1"})
        )
    )
    .unwrap();
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let replies: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let error = |index: usize| replies[index]["result"]["structuredContent"]["errors"][0].clone();
    assert!(error(1)["message"].as_str().unwrap().contains("read-only"));
    assert_eq!(error(1)["code"], "permission_denied");
    assert_eq!(error(2)["code"], "invalid_arguments");
    assert!(
        error(2)["message"]
            .as_str()
            .unwrap()
            .contains("missing string job")
    );
    // Read-only sessions still see writing operations in registry metadata.
    let listed = &replies[3]["result"]["structuredContent"];
    let distance = listed["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["operation"] == "basecraft.distance/tree@1")
        .unwrap();
    assert_eq!(distance["enabled"], true);
    assert_eq!(distance["origin"], "bundled");
    assert_eq!(&replies[4]["result"]["structuredContent"], distance);
    assert_eq!(error(5)["code"], "plugin_unavailable");
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
            json!({"document":0,"tool":"fasttree","output":"fasttree.nwk","params":{}}),
        ),
        request(
            5,
            "run_tool",
            json!({"document":0,"tool":"iqtree","output":"iqtree.nwk","params":{"model":"JC","threads":1,"seed":71}}),
        ),
        request(6, "provenance_inspect", json!({"path":"nj.nwk"})),
        request(7, "provenance_verify", json!({"path":"nj.nwk"})),
        request(8, "replay", json!({})),
        // The descriptor, not a CLI list, decides accepted settings.
        request(
            9,
            "run_tool",
            json!({"document":0,"tool":"fasttree","output":"rejected.nwk","params":{"threads":1}}),
        ),
        request(
            10,
            "run_tool",
            json!({"document":0,"tool":"distance","output":"rejected.nwk","params":{"cache":"x.json"}}),
        ),
        request(
            11,
            "run_tool",
            json!({"document":0,"tool":"delimitation","output":"rejected.json","params":{}}),
        ),
        request(12, "basecraft.distance.missing.v1", json!({})),
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
    assert_eq!(replies.len(), 13);
    for reply in &replies[1..9] {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
    }
    for reply in &replies[9..12] {
        assert_eq!(reply["result"]["isError"], true, "{reply}");
    }
    let unknown = &replies[12]["error"];
    assert_eq!(unknown["code"], -32602, "{unknown}");
    assert_eq!(unknown["data"]["errors"][0]["code"], "plugin_unavailable");
    assert_eq!(unknown["data"]["operation"], "basecraft.distance/missing@1");
    // MCP document-level readers match the CLI commands.
    let cli = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args(args)
            .env("BASECRAFT_PROFILE_DIR", parent.path().join("profile"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let nj = root.join("nj.nwk");
    let inspected: Value =
        serde_json::from_str(&cli(&["provenance", "inspect", nj.to_str().unwrap()])).unwrap();
    assert_eq!(replies[6]["result"]["structuredContent"], inspected);
    assert_eq!(
        replies[7]["result"]["structuredContent"],
        json!({"verified":true,"issues":[]})
    );
    assert!(cli(&["provenance", "verify", nj.to_str().unwrap()]).starts_with("done"));
    assert!(replies[8]["result"]["structuredContent"]["problems"].is_array());
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

#[test]
fn agent_save_as_never_replaces_an_existing_other_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("input.afa"), ">a\nACGT\n>b\nACGT\n").unwrap();
    std::fs::write(d.path().join("other.afa"), ">user\nGGGG\n").unwrap();
    std::fs::create_dir(d.path().join("sub")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["mcp", d.path().to_str().unwrap(), "--allow-writes"])
        .env("BASECRAFT_PROFILE_DIR", d.path().join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        request(1, "open", json!({"path":"input.afa"})),
        request(
            2,
            "apply",
            json!({"document":0,"version":0,"operation":{"op":"aln.replace","params":{"row":0,"start":0,"end":1,"residues":"T"}}}),
        ),
        request(3, "save", json!({"document":0,"path":"other.afa"})),
        request(4, "save", json!({"document":0,"path":"copy.afa"})),
        request(5, "save", json!({"document":0,"path":"sub/../input.afa"})),
    ] {
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
    assert_eq!(replies[3]["result"]["isError"], true, "{}", replies[3]);
    assert_eq!(replies[4]["result"]["isError"], false, "{}", replies[4]);
    assert_eq!(replies[5]["result"]["isError"], false, "{}", replies[5]);
    assert_eq!(
        std::fs::read_to_string(d.path().join("other.afa")).unwrap(),
        ">user\nGGGG\n"
    );
    for saved in ["copy.afa", "input.afa"] {
        assert!(
            std::fs::read_to_string(d.path().join(saved))
                .unwrap()
                .contains("TCGT")
        );
    }
}
