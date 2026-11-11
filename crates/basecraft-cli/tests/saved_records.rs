//! Headless access to saved records the GUI already reads: Sanger reviews
//! (history, methods, verification, packaging, recovery) and support import.
#[path = "../../basecraft-ops/tests/support/sanger_fixture.rs"]
mod sanger_fixture;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use basecraft_ops::sanger_review::{ReviewChoice, ReviewOp, SangerReview};
use basecraft_protocol::Op;
use serde_json::{Value, json};

// Enter the isolated child before creating fixtures: in-process sessions also
// discover AppPaths, and must never consult the parent harness's store registry.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;

fn cli(profile: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .env("BASECRAFT_PROFILE_DIR", profile)
        .output()
        .unwrap()
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn failure(output: &Output) -> String {
    assert!(
        !output.status.success(),
        "unexpected success: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Every file below `root` with its size: reading must leave this unchanged.
fn listing(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let size = std::fs::metadata(&path).unwrap().len();
                found.push((path, size));
            }
        }
    }
    found.sort();
    found
}

fn edit(
    session: &mut basecraft_ops::Session,
    id: basecraft_ops::DocId,
    call_set: &str,
    column: usize,
    base: char,
) {
    session
        .apply(
            id,
            basecraft_protocol::Op::SangerReview {
                call_set: call_set.into(),
                edit: Box::new(ReviewOp::Set {
                    columns: vec![column],
                    choice: ReviewChoice::Edit(base),
                }),
            },
        )
        .unwrap();
}

/// A single-set review and a multi-call-set review of the
/// same instrument trace, the second under a generic `.json` name.
fn reviews(project: &Path) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(project).unwrap();
    let trace = project.join("trace.ab1");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &trace,
    )
    .unwrap();
    let single = basecraft_ops::sanger_review::instrument_review(&trace).unwrap();
    let mut session = basecraft_ops::Session::recoverable(project).unwrap();
    let id = session.create_sanger_review(&single).unwrap();
    edit(&mut session, id, &single.draft_sha256, 0, 'G');
    let v1 = project.join("specimen.sanger-review.json");
    session.save(id, Some(&v1)).unwrap();
    let mut multi = single.clone();
    let mut draft = multi.draft.clone();
    draft.engine = "Tracy".into();
    draft.mode = "basecall".into();
    draft.version_output = "synthetic caller".into();
    draft.engine_sha256 = "a".repeat(64);
    let alternative = SangerReview::from_draft(draft, &multi.draft_path).unwrap();
    let other = alternative.draft_sha256.clone();
    multi.retain_call_set(alternative).unwrap();
    let id = session.create_sanger_review(&multi).unwrap();
    edit(&mut session, id, &multi.draft_sha256, 0, 'G');
    edit(&mut session, id, &other, 0, 'C');
    let v2 = project.join("multi.json");
    session.save(id, Some(&v2)).unwrap();
    (v1, v2)
}

/// The report the GUI shows for a saved review it opened.
fn gui_report(path: &Path) -> Value {
    serde_json::to_value(
        basecraft_ops::document_record::DocumentRecord::saved_report(path).unwrap(),
    )
    .unwrap()
}

struct Mcp {
    child: std::process::Child,
    output: BufReader<std::process::ChildStdout>,
    serial: u64,
}

impl Mcp {
    fn start(project: &Path, profile: &Path, writable: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_basecraft"));
        command
            .args(["mcp", project.to_str().unwrap()])
            .env("BASECRAFT_PROFILE_DIR", profile);
        if writable {
            command.arg("--allow-writes");
        }
        let mut child = command
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

    /// The tool result; JSON-RPC errors (such as an unknown tool) fail the test.
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
fn saved_reviews_report_through_cli_and_mcp_as_in_the_gui() {
    if isolated_profile::enter("saved_reviews_report_through_cli_and_mcp_as_in_the_gui").is_none() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let project = temp.path().join("project");
    let (v1, v2) = reviews(&project);
    let before = listing(&project);
    for path in [&v1, &v2] {
        let expected = gui_report(path);
        let history: Value = serde_json::from_str(&ok(&cli(
            &profile,
            &["history", path.to_str().unwrap(), "--json"],
        )))
        .unwrap();
        assert_eq!(history, expected, "{}", path.display());
        assert_eq!(history["unsaved"], false);
        let methods = ok(&cli(&profile, &["methods", path.to_str().unwrap()]));
        assert_eq!(methods.trim_end(), expected["methods"].as_str().unwrap());
        let text = ok(&cli(&profile, &["history", path.to_str().unwrap()]));
        assert_eq!(text.trim_end(), expected["history_text"].as_str().unwrap());
    }
    // The multi-set report names the other retained call set; v1 has none.
    let multi = gui_report(&v2);
    assert!(
        multi["methods"]
            .as_str()
            .unwrap()
            .contains("Other retained call sets: 1"),
        "{multi}"
    );
    let raw: Value = serde_json::from_str(multi["raw"].as_str().unwrap()).unwrap();
    assert_eq!(raw["call_sets"].as_array().map(Vec::len), Some(1), "{raw}");
    let single = gui_report(&v1);
    assert!(!single["methods"].as_str().unwrap().contains("call set"));

    // Verification and packaging use the shared provenance readers and keep
    // every retained call set.
    ok(&cli(
        &profile,
        &["provenance", "verify", v2.to_str().unwrap()],
    ));
    let package = temp.path().join("package");
    ok(&cli(
        &profile,
        &[
            "provenance",
            "package",
            v2.to_str().unwrap(),
            "--output",
            package.to_str().unwrap(),
        ],
    ));
    let packaged = package.join("multi.json");
    assert_eq!(SangerReview::load(&packaged).unwrap().call_sets().len(), 2);
    let history: Value = serde_json::from_str(&ok(&cli(
        &profile,
        &["history", packaged.to_str().unwrap(), "--json"],
    )))
    .unwrap();
    assert_eq!(history, gui_report(&packaged));

    // Read-only MCP can inspect a review through the common document interface.
    let mut mcp = Mcp::start(&project, &profile, false);
    let record = mcp.tool("file_record", &json!({"path":"multi.json"}));
    assert_eq!(record["isError"], false, "{record}");
    assert_eq!(record["structuredContent"], gui_report(&v2));
    let opened = mcp.tool("open", &json!({"path":"multi.json"}));
    assert_eq!(opened["isError"], false, "{opened}");
    assert_eq!(opened["structuredContent"]["kind"], "sanger_review");
    mcp.close();

    // CLI editing is refused explicitly and leaves the review unchanged.
    let bytes = std::fs::read(&v2).unwrap();
    let error = failure(&cli(
        &profile,
        &[
            "edit",
            v2.to_str().unwrap(),
            r#"{"op":"seq.replace","params":{"start":0,"end":1,"residues":"T"}}"#,
        ],
    ));
    assert!(error.contains("Sanger"), "{error}");
    assert_eq!(std::fs::read(&v2).unwrap(), bytes);
    // Reading created no records next to the reviews.
    assert_eq!(listing(&project), before);
}

#[test]
fn damaged_unknown_future_and_draft_files_are_refused_without_records() {
    if isolated_profile::enter("damaged_unknown_future_and_draft_files_are_refused_without_records")
        .is_none()
    {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let project = temp.path().join("project");
    let (v1, _) = reviews(&project);
    let bytes = std::fs::read(&v1).unwrap();
    let mut future: Value = serde_json::from_slice(&bytes).unwrap();
    future["version"] = json!(99);
    let draft = SangerReview::load(&v1).unwrap().draft;
    let cases = [
        (
            "unrelated.sanger-review.json",
            br#"{"other":true}"#.to_vec(),
        ),
        ("truncated.json", bytes[..bytes.len() / 2].to_vec()),
        ("future.json", serde_json::to_vec(&future).unwrap()),
        ("draft.json", serde_json::to_vec(&draft).unwrap()),
        ("sequence.sanger-review.json", b">a\nACGT\n".to_vec()),
    ];
    for (name, content) in &cases {
        std::fs::write(project.join(name), content).unwrap();
    }
    let before = listing(&project);
    for (name, _) in &cases {
        let path = project.join(name);
        for command in ["history", "methods"] {
            let error = failure(&cli(&profile, &[command, path.to_str().unwrap()]));
            assert!(error.contains("unknown_format"), "{name}: {error}");
        }
    }
    assert_eq!(listing(&project), before);
}

fn trees(project: &Path) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(project).unwrap();
    let target = project.join("target.nwk");
    std::fs::write(&target, "((A:1,B:1)90:1,(C:1,D:1)80:1);").unwrap();
    let source = project.join("support.nwk");
    std::fs::write(&source, "((A:1,B:1)75:1,(C:1,D:1)60:1);").unwrap();
    (target, source)
}

fn import_event(target: &Path) -> Value {
    let publication = basecraft_ops::provenance::inspect(target).unwrap();
    let params = publication
        .events
        .iter()
        .find_map(|event| match &event.op {
            Op::ToolRun {
                plugin,
                operation,
                params,
                ..
            } if plugin == "basecraft.tree" && operation == "import_support" => {
                Some(params.clone())
            }
            _ => None,
        })
        .expect("import activity");
    assert!(params["input_run"]["id"].is_string(), "{params}");
    params
}

#[test]
fn support_import_runs_headlessly_with_the_same_reference_evidence() {
    if isolated_profile::enter("support_import_runs_headlessly_with_the_same_reference_evidence")
        .is_none()
    {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");

    // CLI: import, save, verify; a replaced source is reported.
    let (target, source) = trees(&temp.path().join("cli"));
    let imported: Value = serde_json::from_str(&ok(&cli(
        &profile,
        &[
            "import-support",
            target.to_str().unwrap(),
            source.to_str().unwrap(),
            "--name",
            "Bootstrap",
        ],
    )))
    .unwrap();
    assert_eq!(imported, json!({"added":["Bootstrap"],"matched":2}));
    assert_eq!(import_event(&target)["sets"], json!(["Bootstrap"]));
    ok(&cli(
        &profile,
        &["provenance", "verify", target.to_str().unwrap()],
    ));
    let history = ok(&cli(&profile, &["history", target.to_str().unwrap()]));
    assert!(
        history.contains("Ran basecraft.tree import_support"),
        "{history}"
    );
    assert!(!history.contains(", wrote "), "{history}");
    let reopened = basecraft_ops::files::load_tree(&target).unwrap();
    assert!(reopened.doc.support("Bootstrap").is_some());
    std::fs::write(&source, "((A:1,B:1)10:1,(C:1,D:1)20:1);").unwrap();
    failure(&cli(
        &profile,
        &["provenance", "verify", target.to_str().unwrap()],
    ));

    // MCP: the same import on an open tree; read-only servers refuse it.
    let project = temp.path().join("mcp");
    let (target, _) = trees(&project);
    let mut readonly = Mcp::start(&project, &profile, false);
    readonly.tool("open", &json!({"path":"target.nwk"}));
    let denied = readonly.tool(
        "import_support",
        &json!({"document":0,"path":"support.nwk","name":"Bootstrap"}),
    );
    assert_eq!(denied["isError"], true, "{denied}");
    assert_eq!(
        denied["structuredContent"]["errors"][0]["code"],
        "permission_denied"
    );
    readonly.close();
    let mut mcp = Mcp::start(&project, &profile, true);
    mcp.tool("open", &json!({"path":"target.nwk"}));
    let outside = mcp.tool(
        "import_support",
        &json!({"document":0,"path":"/etc/hostname","name":"Bootstrap"}),
    );
    assert_eq!(outside["isError"], true, "{outside}");
    let imported = mcp.tool(
        "import_support",
        &json!({"document":0,"path":"support.nwk","name":"Bootstrap"}),
    );
    assert_eq!(imported["isError"], false, "{imported}");
    assert_eq!(imported["structuredContent"]["added"], json!(["Bootstrap"]));
    assert_eq!(imported["structuredContent"]["matched"], 2);
    let saved = mcp.tool("save", &json!({"document":0}));
    assert_eq!(saved["isError"], false, "{saved}");
    mcp.close();
    let params = import_event(&target);
    assert_eq!(params["sets"], json!(["Bootstrap"]));
    assert_eq!(
        basecraft_ops::provenance::verify(&target).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn sequence_extraction_cli_mcp_and_python_share_source_and_mapping_semantics() {
    if isolated_profile::enter(
        "sequence_extraction_cli_mcp_and_python_share_source_and_mapping_semantics",
    )
    .is_none()
    {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let profile = temp.path().join("profile");
    std::fs::create_dir(&project).unwrap();
    let source = project.join("gene.fasta");
    std::fs::write(&source, ">gene\nACGTACGT\n").unwrap();
    let output = project.join("cli.gb");
    ok(&cli(
        &profile,
        &[
            "extract",
            source.to_str().unwrap(),
            "1",
            "7",
            "--segments",
            "[[1,3],[5,7]]",
            "--reverse",
            "--output",
            output.to_str().unwrap(),
        ],
    ));
    let expected = std::fs::read(&output).unwrap();
    let mut mcp = Mcp::start(&project, &profile, true);
    let opened = mcp.tool("open", &json!({"path":"gene.fasta"}));
    let doc = opened["structuredContent"]["document"].clone();
    let version = opened["structuredContent"]["version"].clone();
    let mut args = json!({"document":doc,"version":version,"path":"mcp.gb","start":1,"end":7,"segments":[[1,3],[5,7]],"reverse":true});
    let extracted = mcp.tool("extract", &args);
    assert_ne!(extracted["isError"], true, "{extracted}");
    assert_eq!(std::fs::read(project.join("mcp.gb")).unwrap(), expected);
    assert_eq!(
        mcp.tool("extract", &args)["isError"],
        true,
        "existing destination was replaced"
    );
    args["path"] = json!("stale.gb");
    args["version"] = json!(999);
    assert_eq!(mcp.tool("extract", &args)["isError"], true);
    assert!(!project.join("stale.gb").exists());
    mcp.close();
    let mut readonly = Mcp::start(&project, &profile, false);
    let opened = readonly.tool("open", &json!({"path":"gene.fasta"}));
    args["document"] = opened["structuredContent"]["document"].clone();
    args["version"] = opened["structuredContent"]["version"].clone();
    assert_eq!(readonly.tool("extract", &args)["isError"], true);
    readonly.close();
    for name in ["cli.gb", "mcp.gb"] {
        let result = project.join(name);
        let publication = basecraft_ops::provenance::inspect(&result).unwrap();
        assert!(publication.inputs[0].reference.is_some());
        let input = basecraft_ops::input_evidence::read_input(
            &basecraft_ops::provenance::bundle_for_result(&result),
        )
        .unwrap();
        assert_eq!(
            input.extra["extraction"]["selection"]["params"]["segments"],
            json!([[1, 3], [5, 7]])
        );
    }
    // Python delegates directly to this same MCP method, including its writes gate.
    let python = std::env::var("BASECRAFT_PYTHON").unwrap_or_else(|_| "python3".into());
    let script = r"
from basecraft.session import Session
s = object.__new__(Session)
s.call = lambda name, args: (name,args)
name,args = s.extract(3,'new.gb',1,7,version=4,segments=[(1,3),(5,7)],reverse=True)
assert name == 'extract' and args['version'] == 4 and args['reverse']
assert args['segments'] == [(1,3),(5,7)]
";
    assert!(
        Command::new(python)
            .args(["-c", script])
            .env(
                "PYTHONPATH",
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src")
            )
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn sanger_edits_share_cli_mcp_history_and_undo() {
    let Some(root) = isolated_profile::enter("sanger_edits_share_cli_mcp_history_and_undo") else {
        return;
    };
    let project = root.join("project");
    let profile = root.join("profile");
    std::fs::create_dir_all(&project).unwrap();
    let path = sanger_fixture::copy_to(&project, "multi.json");
    let call_set = SangerReview::load(&path).unwrap().draft_sha256;
    let operation = Op::SangerReview {
        call_set: call_set.clone(),
        edit: Box::new(ReviewOp::Set {
            columns: vec![0],
            choice: ReviewChoice::Edit('N'),
        }),
    };
    ok(&cli(
        &profile,
        &[
            "edit",
            path.to_str().unwrap(),
            &serde_json::to_string(&[
                operation.clone(),
                Op::Undo { of: String::new() },
                Op::Redo { of: String::new() },
            ])
            .unwrap(),
        ],
    ));
    assert_eq!(
        SangerReview::load(&path).unwrap().decision(0),
        Some(&ReviewChoice::Edit('N'))
    );
    let history = ok(&cli(
        &profile,
        &["history", path.to_str().unwrap(), "--json"],
    ));
    assert!(history.contains("Sanger review ") && history.contains(&call_set));
    let mut mcp = Mcp::start(&project, &profile, true);
    let opened = mcp.tool("open", &json!({"path":"multi.json"}));
    assert_eq!(opened["isError"], false, "{opened}");
    let state = &opened["structuredContent"];
    let id = state["document"].clone();
    let edited = mcp.tool("apply", &json!({"document":id,"version":state["version"],"operation": Op::SangerReview { call_set, edit: Box::new(ReviewOp::Set { columns: vec![0], choice: ReviewChoice::Edit('A') }) }}));
    assert_eq!(edited["isError"], false, "{edited}");
    let undone = mcp.tool("undo", &json!({"document":id}));
    assert_eq!(undone["isError"], false, "{undone}");
    assert_eq!(undone["structuredContent"]["decisions"][0]["base"], "N");
    let redone = mcp.tool("redo", &json!({"document":id}));
    assert_eq!(redone["isError"], false, "{redone}");
    let saved = mcp.tool("save", &json!({"document":id}));
    assert_eq!(saved["isError"], false, "{saved}");
    mcp.close();
    let reopened = SangerReview::load(&path).unwrap();
    assert_eq!(reopened.call_sets().len(), 2);
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('A')));
}
