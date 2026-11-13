//! The same annotation through every route that applies sequence proposals
//! (GUI job runner, CLI `plugin`, MCP operation tool)
//! leaves the same invocation evidence in the saved record.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use basecraft_ops::{Session, records};
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::{LogEntry, Op, operation::RuntimeState};
use serde_json::{Value, json};

fn python_ready() -> bool {
    let python = basecraft_plugin_host::process::python_interpreter();
    let ok = Command::new(&python)
        .args(["-c", "import pyarrow, numpy"])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipping: {} lacks pyarrow or numpy", python.display());
    }
    ok
}

fn bundled() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .canonicalize()
        .unwrap()
}

/// One ORF of 120 codons; the same input for every route.
fn project(root: &Path, name: &str) -> PathBuf {
    let project = root.join(name);
    std::fs::create_dir_all(&project).unwrap();
    let file = project.join("orf.fasta");
    let mut seq = "C".repeat(10);
    seq.push_str("ATG");
    seq.push_str(&"AAA".repeat(120));
    seq.push_str("TAA");
    seq.push_str(&"C".repeat(200));
    std::fs::write(&file, format!(">orf\n{seq}\n")).unwrap();
    file
}

fn cli(profile: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_basecraft"));
    command
        .env("BASECRAFT_PROFILE_DIR", profile)
        .env("BASECRAFT_PLUGINS", bundled());
    command
}

fn tool_run(file: &Path, plugin: &str) -> LogEntry {
    let publication = records::inspect(file, Some(&project_of(file))).unwrap();
    let mut runs: Vec<_> = publication
        .events
        .into_iter()
        .filter(|event| matches!(&event.op, Op::ToolRun { plugin: p, .. } if p == plugin))
        .collect();
    assert_eq!(runs.len(), 1, "{}", file.display());
    runs.remove(0)
}

/// Route-independent invocation evidence: the operation, accepted package,
/// requested and effective settings, and the hash of the submitted snapshot.
fn invocation(file: &Path, entry: &LogEntry) -> Value {
    let Op::ToolRun {
        plugin,
        operation,
        params,
        ..
    } = &entry.op
    else {
        unreachable!()
    };
    let record = run_record(file, entry);
    json!({"actor": entry.actor, "plugin": plugin, "operation": operation,
        "requested": params, "effective": record.effective_parameters,
        "id": record.operation, "pins": [record.package_digest, record.descriptor_digest],
        "snapshot": entry.result.hashes.iter().map(|(_, hash)| hash).collect::<Vec<_>>()})
}

/// The run record the entry points at, in the project's run folder.
fn run_record(file: &Path, entry: &LogEntry) -> basecraft_protocol::records::RunRecord {
    let Op::ToolRun { run: Some(run), .. } = &entry.op else {
        panic!("the tool run names no run folder")
    };
    let folder = project_of(file).root().join(&run.folder);
    serde_json::from_slice(&std::fs::read(folder.join("execution.json")).unwrap()).unwrap()
}

fn features(file: &Path) -> Vec<(String, String)> {
    let basecraft_ops::files::Loaded::Sequence(opened) = basecraft_ops::files::load(file).unwrap()
    else {
        panic!("not a sequence")
    };
    opened
        .doc
        .features()
        .map(|(_, f)| (f.kind.clone(), format!("{:?}", f.location)))
        .collect()
}

struct Mcp {
    child: std::process::Child,
    output: BufReader<std::process::ChildStdout>,
    serial: u64,
}

impl Mcp {
    fn start(project: &Path, profile: &Path) -> Self {
        let mut child = cli(profile)
            .args(["mcp", project.to_str().unwrap(), "--allow-writes"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut mcp = Self {
            child,
            output,
            serial: 0,
        };
        mcp.call(
            "initialize",
            &json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        );
        mcp
    }

    fn call(&mut self, method: &str, params: &Value) -> Value {
        self.serial += 1;
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            json!({"jsonrpc":"2.0","id":self.serial,"method":method,"params":params})
        )
        .unwrap();
        stdin.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(self.output.read_line(&mut line).unwrap() > 0, "MCP closed");
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == json!(self.serial) {
                return reply;
            }
        }
    }

    fn tool(&mut self, name: &str, arguments: &Value) -> Value {
        let reply = self.call("tools/call", &json!({"name":name,"arguments":arguments}));
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    fn close(mut self) {
        drop(self.child.stdin.take());
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn every_annotation_route_keeps_the_same_invocation_evidence() {
    crate::isolated_profile::isolate!();
    if !python_ready() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");

    // GUI: the registered runner against an open document, skipping known features.
    let gui = project(temp.path(), "gui");
    let paths = basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| profile.clone().into_os_string())
    })
    .unwrap();
    let registry = Registry::load(&RegistryConfig {
        bundled: bundled(),
        packages: profile.join("plugins"),
        index: profile.join("index.json"),
    })
    .unwrap();
    let mut session = Session::recoverable_with_paths(gui.parent().unwrap(), &paths).unwrap();
    let doc = session.open(&gui).unwrap();
    let mut jobs = JobManager::default().skipping_known_features();
    let request = serde_json::from_value(json!({
        "operation": "basecraft.orf-finder/find_orfs@1",
        "inputs": {"sequence": [{"kind": "document", "doc": doc.0.to_string(),
            "generation": session.generation(doc).unwrap(),
            "revision": session.checked_sequence(doc).unwrap().version()}]},
        "parameters": {"min_codons": 100, "genetic_code": 11}}))
    .unwrap();
    let policy = SessionPolicy::new(session.root(), true).unwrap();
    let accepted = jobs
        .submit_in_session(&registry, request, policy, "GUI", &mut session)
        .unwrap();
    let outcome = jobs
        .wait_in_session(&accepted.run_id, &mut session, &|| false)
        .unwrap();
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:?}");
    session.save(doc, None).unwrap();
    drop(session);

    // CLI `plugin` with a bundled schema-2 package.
    let script = project(temp.path(), "cli");
    let output = cli(&profile)
        .args([
            "plugin",
            bundled().join("orf-finder").to_str().unwrap(),
            script.to_str().unwrap(),
            "find_orfs",
            "min_codons=100",
            "genetic_code=11",
            "--save",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // MCP: the generated operation tool on an open document, then save.
    let agent = project(temp.path(), "mcp");
    let mut mcp = Mcp::start(agent.parent().unwrap(), &profile);
    let opened = mcp.tool("open", &json!({"path":"orf.fasta"}))["structuredContent"].clone();
    let accepted = mcp.tool(
        "basecraft.orf-finder.find_orfs.v1",
        &json!({"inputs":{"sequence":[{"kind":"document","doc":opened["document"].to_string(),
            "generation":opened["generation"],"revision":opened["version"]}]},
            "parameters":{"min_codons":100,"genetic_code":11}}),
    )["structuredContent"]
        .clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let outcome = mcp.tool(
            "basecraft.host.jobs_get.v1",
            &json!({"job":accepted["run_id"]}),
        )["structuredContent"]
            .clone();
        if outcome["state"] == "completed" {
            break;
        }
        assert!(
            !matches!(
                outcome["state"].as_str(),
                Some("failed" | "partial" | "cancelled")
            ),
            "{outcome}"
        );
        assert!(std::time::Instant::now() < deadline, "{outcome}");
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let saved = mcp.tool("save", &json!({"document":opened["document"]}));
    assert_eq!(saved["isError"], false, "{saved}");
    mcp.close();

    let expected = invocation(&gui, &tool_run(&gui, "basecraft.orf-finder"));
    assert_eq!(expected["effective"]["min_codons"], 100, "{expected}");
    assert_eq!(expected["effective"]["genetic_code"], 11, "{expected}");
    assert_eq!(expected["snapshot"].as_array().unwrap().len(), 1);
    for (file, initiator) in [(&gui, "GUI"), (&script, "CLI"), (&agent, "mcp-client")] {
        let entry = tool_run(file, "basecraft.orf-finder");
        assert_eq!(invocation(file, &entry), expected, "{}", file.display());
        assert_eq!(run_record(file, &entry).initiator, initiator);
        assert_eq!(features(file), features(&gui), "{}", file.display());
        assert_eq!(features(file).len(), 1);
        assert_eq!(
            records::verify(file, Some(&project_of(file))).unwrap(),
            Vec::<String>::new(),
            "{}",
            file.display()
        );
    }
}

/// A schema-1 package has no typed contract: refuse it before executing Python
/// or opening/mutating the document.
#[test]
fn schema_one_plugin_call_is_rejected_without_execution_or_mutation() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let package = temp.path().join("old-package");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(
        package.join("plugin.toml"),
        "schema = 1\nid = \"org.example.old\"\nname = \"Old\"\nversion = \"1.0.0\"\n[entry]\noperation = \"python:plugin.py\"\n",
    )
    .unwrap();
    let sentinel = package.join("executed");
    std::fs::write(
        package.join("plugin.py"),
        "from pathlib import Path\nPath(__file__).with_name('executed').touch()\n",
    )
    .unwrap();
    let file = project(temp.path(), "old-project");
    let original = std::fs::read(&file).unwrap();
    let output = cli(&profile)
        .args([
            "plugin",
            package.to_str().unwrap(),
            file.to_str().unwrap(),
            "noop",
            "end=4",
            "--save",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("unsupported schema 1"), "{error}");
    assert!(
        !sentinel.exists(),
        "Rejected package entry point was executed"
    );
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert!(!file.parent().unwrap().join(".basecraft").exists());
    assert!(!records::companion(&file).exists());
}

/// The tests' MCP sessions use the file's folder as their project.
fn project_of(file: &std::path::Path) -> basecraft_ops::project::Project {
    basecraft_ops::project::Project::new(file.parent().unwrap()).unwrap()
}
