use basecraft_ops::links::AlignmentLinkExt;
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

/// A synchronous MCP client for one server process, waiting on job results.
struct Client {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
    next: u32,
}

impl Client {
    fn start(root: &std::path::Path, profile: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
            .args(["mcp", root.to_str().unwrap(), "--allow-writes"])
            .env("BASECRAFT_PROFILE_DIR", profile)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            input,
            output,
            next: 1,
        };
        client.send(&json!({"jsonrpc":"2.0","id":0,"method":"initialize"}), 0);
        client
    }

    /// Send one message and return the reply with its ID, skipping notifications.
    fn send(&mut self, message: &Value, id: u32) -> Value {
        use std::io::BufRead;
        writeln!(self.input, "{message}").unwrap();
        loop {
            let mut line = String::new();
            assert_ne!(
                self.output.read_line(&mut line).unwrap(),
                0,
                "server exited"
            );
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == id {
                return reply;
            }
        }
    }

    fn call(&mut self, name: &str, args: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&request(id, name, args), id)
    }

    /// Submit an operation tool call and wait for the job's terminal outcome.
    fn run(&mut self, name: &str, args: Value) -> Value {
        let accepted = self.call(name, args);
        assert_eq!(accepted["result"]["isError"], false, "{accepted}");
        let job = accepted["result"]["structuredContent"]["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let status = self.call("basecraft.host.jobs_get.v1", json!({"job":job}));
            let outcome = status["result"]["structuredContent"].clone();
            if ["completed", "failed", "cancelled", "interrupted", "partial"]
                .contains(&outcome["state"].as_str().unwrap_or(""))
            {
                return outcome;
            }
            assert!(std::time::Instant::now() < deadline, "job did not finish");
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    fn close(self) {
        drop(self.input);
        let mut child = self.child;
        assert!(child.wait().unwrap().success());
    }
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
    let mut client = Client::start(&root, &parent.path().join("profile"));
    let created = client.run(
        "basecraft.delimitation.workspace_tree.v1",
        json!({"inputs":{"tree":[{"kind":"file","path":"source.nwk"}]},"parameters":{},
            "destinations":{"workspace":"species.spart"}}),
    );
    assert_eq!(created["state"], "completed", "{created}");
    // A workspace named by path is opened, receives the hypotheses and is saved.
    let fitted = client.run(
        "basecraft.delimitation.gmyc_single.v1",
        json!({"inputs":{"tree":[{"kind":"file","path":"source.nwk"}]},"parameters":{"rooted":true},
            "target":{"kind":"file","path":"species.spart"}}),
    );
    assert_eq!(fitted["state"], "completed", "{fitted}");
    client.close();
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
    let mut client = Client::start(&root, &parent.path().join("profile"));
    let opened = client.call("open", json!({"path":"source.afa"}));
    assert_eq!(opened["result"]["isError"], false, "{opened}");
    let state = client.call("state", json!({"document":0}));
    let state = &state["result"]["structuredContent"];
    // The open document is the input; each run lands in its history.
    let document = json!([{"kind":"document","doc":"0","generation":state["generation"],"revision":state["version"]}]);
    for (tool, parameters, output) in [
        (
            "basecraft.distance.tree.v1",
            json!({"method":"sorted","model":"p","threads":1}),
            "nj.nwk",
        ),
        (
            "basecraft.distance.tree.v1",
            json!({"method":"upgma","model":"p","threads":1}),
            "upgma.nwk",
        ),
        ("basecraft.fasttree.tree.v1", json!({}), "fasttree.nwk"),
        (
            "basecraft.iqtree.tree.v1",
            json!({"model":"JC","threads":1,"seed":71}),
            "iqtree.nwk",
        ),
    ] {
        let outcome = client.run(
            tool,
            json!({"inputs":{"alignment":document},"parameters":parameters,
                "destinations":{"artifacts/tree.nwk":output}}),
        );
        assert_eq!(outcome["state"], "completed", "{tool}: {outcome}");
    }
    let record = client.call("document_record", json!({"document":0}));
    let record = record["result"]["structuredContent"].to_string();
    assert_eq!(record.matches("params / run id").count(), 4, "{record}");
    // The descriptor, not a route-specific list, decides accepted settings.
    for (tool, parameters) in [
        ("basecraft.fasttree.tree.v1", json!({"threads":1})),
        ("basecraft.distance.tree.v1", json!({"cache":"x.json"})),
    ] {
        let refused = client.call(
            tool,
            json!({"inputs":{"alignment":document},"parameters":parameters}),
        );
        assert_eq!(refused["result"]["isError"], true, "{refused}");
    }
    let unknown = client.call("basecraft.distance.missing.v1", json!({}));
    let unknown = &unknown["error"];
    assert_eq!(unknown["code"], -32602, "{unknown}");
    assert_eq!(unknown["data"]["errors"][0]["code"], "plugin_unavailable");
    assert_eq!(unknown["data"]["operation"], "basecraft.distance/missing@1");
    let inspected_mcp = client.call("provenance_inspect", json!({"path":"nj.nwk"}));
    let verified_mcp = client.call("provenance_verify", json!({"path":"nj.nwk"}));
    let replayed = client.call("replay", json!({}));
    client.close();
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
    assert_eq!(inspected_mcp["result"]["structuredContent"], inspected);
    assert_eq!(
        verified_mcp["result"]["structuredContent"],
        json!({"verified":true,"issues":[]})
    );
    assert!(cli(&["provenance", "verify", nj.to_str().unwrap()]).starts_with("done"));
    assert!(replayed["result"]["structuredContent"]["problems"].is_array());
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
    for name in ["fasttree.nwk", "iqtree.nwk", "nj.nwk", "upgma.nwk"] {
        let output = root.join(name);
        let run = basecraft_ops::analysis_store::ArtifactRun::for_result(&output)
            .unwrap()
            .unwrap();
        assert!(run.join("execution.json").is_file());
        assert!(basecraft_ops::links::AlignmentLink::read(&output).is_ok());
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

fn session(dir: &std::path::Path, writes: bool, messages: &[Value]) -> Vec<Value> {
    let mut args = vec!["mcp".to_string(), dir.to_string_lossy().into_owned()];
    if writes {
        args.push("--allow-writes".into());
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(&args)
        .env("BASECRAFT_PROFILE_DIR", dir.join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})
    )
    .unwrap();
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    for message in messages {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .skip(1)
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[test]
fn metadata_tools_respect_write_permission_and_plan_before_applying() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("rbcL.fasta"), ">HV1|Codium_fragile\nACGT\n").unwrap();
    let readonly = session(
        d.path(),
        false,
        &[
            request(1, "metadata_open", json!({})),
            request(2, "metadata_open", json!({"create":true})),
        ],
    );
    assert_eq!(readonly[0]["result"]["isError"], true);
    let text = readonly[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("permission_denied"), "{text}");
    assert!(!d.path().join("metadata").exists());
    let pattern =
        json!({"name":"p","separator":"|","fields":[{"role":"specimen"},{"role":"taxon"}]});
    let replies = session(
        d.path(),
        true,
        &[
            request(1, "metadata_open", json!({"create":true})),
            request(
                2,
                "metadata_plan_headers",
                json!({"document":0,"request":{"document":"rbcL.fasta","pattern":pattern}}),
            ),
            request(3, "read", json!({"document":0})),
            request(
                4,
                "metadata_plan_headers",
                json!({"document":0,"request":{"document":"../outside.fasta","pattern":pattern}}),
            ),
        ],
    );
    assert_eq!(
        replies[0]["result"]["structuredContent"]["kind"],
        "metadata"
    );
    let plan = &replies[1]["result"]["structuredContent"];
    assert_eq!(plan["changes"]["specimens"], json!(["HV1"]));
    assert_eq!(plan["changes"]["taxa"][0]["name"], "Codium fragile");
    // Planning applies nothing.
    let read = &replies[2]["result"]["structuredContent"];
    assert_eq!(read["metadata"]["specimens"]["rows"], json!([]));
    assert_eq!(replies[3]["result"]["isError"], true);
}

#[test]
fn submission_tools_respect_write_permission_and_edit_through_apply() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("its.afa"), ">a\nACGTACGT\n>b\nACGTAC--\n").unwrap();
    let readonly = session(
        d.path(),
        false,
        &[
            request(
                1,
                "submission_create",
                json!({"path":"its.submission.json","alignment":"its.afa"}),
            ),
            request(2, "submission_submitter", json!({"set":{"city":"Hobart"}})),
            request(3, "submission_inclusion_presets", json!({"save":{}})),
        ],
    );
    for reply in &readonly {
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("permission_denied"), "{text}");
    }
    assert!(!d.path().join("its.submission.json").exists());
    let edit = json!({"action":"set_rows","rows":[{"record":"b","decision":"exclude"}]});
    let replies = session(
        d.path(),
        true,
        &[
            request(
                1,
                "submission_create",
                json!({"path":"its.submission.json","alignment":"its.afa"}),
            ),
            request(
                2,
                "apply",
                json!({"document":0,"version":0,"operation":{"op":"submission.edit","params":{"edit":edit}}}),
            ),
            request(3, "read", json!({"document":0})),
            request(4, "submission_review", json!({"document":0})),
            request(5, "submission_submitter", json!({})),
        ],
    );
    assert_eq!(
        replies[0]["result"]["structuredContent"]["kind"],
        "submission"
    );
    let read = &replies[2]["result"]["structuredContent"];
    assert_eq!(read["rows"][0]["decision"], "exclude");
    let review = &replies[3]["result"]["structuredContent"];
    assert_eq!(review["records"][1]["decision"]["decision"], "exclude");
    assert!(
        review["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "unlinked")
    );
    assert!(
        replies[4]["result"]["structuredContent"]["missing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "email")
    );
}

#[test]
fn submission_selection_confirmation_is_shared_by_mcp_export_and_apply() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.afa"), ">local\nACGT\n>MZ123456.1\nACGT\n").unwrap();
    let replies = session(
        d.path(),
        true,
        &[
            request(
                1,
                "submission_create",
                json!({"path":"a.submission.json", "alignment":"a.afa"}),
            ),
            request(2, "submission_review", json!({"document":0})),
            request(
                3,
                "submission_export",
                json!({"document":0,"output":"package"}),
            ),
            request(4, "save", json!({"document":0})),
        ],
    );
    let review = &replies[1]["result"]["structuredContent"];
    assert_eq!(review["selection_confirmed"], false);
    assert_eq!(review["records"][1]["decision"]["decision"], "exclude");
    assert!(replies[2]["result"]["isError"].as_bool().unwrap_or(false));
    assert!(!d.path().join("package").exists());
    let hash = review["selection_hash"].as_str().unwrap();
    let replies = session(
        d.path(),
        true,
        &[
            request(1, "open", json!({"path":"a.submission.json"})),
            request(
                2,
                "apply",
                json!({"document":0,"version":0,
            "operation":{"op":"submission.edit","params":{"edit":{
                "action":"confirm_selection","selection_hash":hash}}}}),
            ),
            request(3, "submission_review", json!({"document":0})),
            request(
                4,
                "submission_export",
                json!({"document":0,"output":"package"}),
            ),
        ],
    );
    assert_eq!(
        replies[2]["result"]["structuredContent"]["selection_confirmed"],
        true
    );
    assert_eq!(replies[3]["result"]["structuredContent"]["records"], 1);
    assert!(d.path().join("package/package.json").is_file());
}

#[test]
fn wrong_kind_requests_fail_without_mutation_and_the_server_remains_usable() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("input.afa");
    let original = b">a\nACGT\n>b\nACGT\n";
    std::fs::write(&file, original).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["mcp", dir.path().to_str().unwrap(), "--allow-writes"])
        .env("BASECRAFT_PROFILE_DIR", dir.path().join("profile"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in [
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"checked-access","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        request(1, "open", json!({"path":"input.afa"})),
        request(
            2,
            "metadata_status",
            json!({"document":0,"file":"input.afa"}),
        ),
        request(
            3,
            "import_support",
            json!({"document":0,"path":"input.afa"}),
        ),
        request(4, "read", json!({"document":999,"row":0,"start":0,"end":4})),
        request(5, "read", json!({"document":0,"row":0,"start":0,"end":4})),
        request(6, "state", json!({"document":0})),
    ] {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    for reply in &replies[2..=4] {
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(
            reply["result"]["structuredContent"]["errors"][0]["code"],
            "invalid_arguments"
        );
    }
    assert_eq!(
        replies[5]["result"]["structuredContent"]["residues"],
        "ACGT"
    );
    assert_eq!(replies[6]["result"]["structuredContent"]["version"], 0);
    assert_eq!(std::fs::read(file).unwrap(), original);
}

#[test]
fn cli_submission_show_rejects_an_alignment_without_success_output() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("input.afa");
    let original = b">a\nACGT\n>b\nACGT\n";
    std::fs::write(&file, original).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(["submission", "show"])
        .arg(&file)
        .arg("--project")
        .arg(dir.path())
        .env("BASECRAFT_PROFILE_DIR", dir.path().join("profile"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Submission"));
    assert_eq!(std::fs::read(file).unwrap(), original);
}
