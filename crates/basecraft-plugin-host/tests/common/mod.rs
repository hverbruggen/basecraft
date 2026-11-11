//! Registered-operation helpers shared by the plugin-host integration tests.
//! Every analysis runs through `JobManager`, as the GUI, CLI, Python and MCP do.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use basecraft_ops::{DocId, Session};
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::operation::{OperationOutcome, OperationRequest, RuntimeState};
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(300);

/// This checkout's bundled plugin packages.
pub fn plugins() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

/// A registry of the packages in `bundled`, with no installed profile packages.
pub fn registry(bundled: &Path) -> Registry {
    let profile = Path::new("/nonexistent/basecraft-test-profile");
    Registry::load(&RegistryConfig {
        bundled: bundled.into(),
        packages: profile.join("plugins"),
        index: profile.join("index.json"),
    })
    .unwrap()
}

/// The registry of this checkout's bundled packages.
pub fn bundled() -> Registry {
    registry(&plugins())
}

/// A bundled-package folder under `root` holding one package: `source`'s
/// manifest identity and its `operation` descriptor, implemented by `script`.
/// Storage tests use it to exercise publication without a scientific tool.
pub fn fixture_package(root: &Path, source: &str, operation: &str, script: &str) -> PathBuf {
    let bundled = root.join("fixture-packages");
    let package = bundled.join(source);
    std::fs::create_dir_all(&package).unwrap();
    let mut contract: Value = serde_json::from_slice(
        &std::fs::read(plugins().join(source).join("operations.json")).unwrap(),
    )
    .unwrap();
    contract["operations"]
        .as_array_mut()
        .unwrap()
        .retain(|descriptor| descriptor["name"] == operation);
    std::fs::write(
        package.join("operations.json"),
        serde_json::to_vec_pretty(&contract).unwrap(),
    )
    .unwrap();
    std::fs::write(
        package.join("plugin.toml"),
        format!(
            "schema = 2\ncontract = \"operations.json\"\nid = \"basecraft.{source}\"\nname = \"Storage fixture\"\nversion = \"test\"\n[entry]\noperation = \"python:fixture.py\"\n"
        ),
    )
    .unwrap();
    std::fs::write(package.join("fixture.py"), script).unwrap();
    bundled
}

/// A request input naming `doc` at its current generation and revision.
pub fn document(session: &Session, doc: DocId) -> Value {
    json!([{"kind":"document","doc":doc.0.to_string(),
        "generation":session.generation(doc),"revision":session.revision(doc)}])
}

/// A request input naming a project file.
pub fn file(path: &str) -> Value {
    json!([{"kind":"file","path":path}])
}

/// Submit `request` in `session`, rooted at the session's folder.
pub fn submit_in(
    manager: &mut JobManager,
    registry: &Registry,
    session: &mut Session,
    request: impl FnOnce(&Session) -> Value,
) -> basecraft_plugin_host::Result<OperationOutcome> {
    let request: OperationRequest = serde_json::from_value(request(session)).unwrap();
    let policy = SessionPolicy::new(session.root(), true).unwrap();
    manager.submit_in_session(registry, request, policy, "test", session)
}

/// Poll the owner session until the run is terminal, as the GUI bridges do.
pub fn wait_in(manager: &JobManager, session: &mut Session, id: &str) -> OperationOutcome {
    let deadline = Instant::now() + DEADLINE;
    loop {
        manager.poll_session(session);
        let outcome = manager.get(id).unwrap();
        if outcome.state.terminal() {
            return outcome;
        }
        assert!(Instant::now() < deadline, "run {id} did not terminate");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Run `request` in `session` to a terminal outcome.
/// `request` builds the request from the session, so it can name its documents.
pub fn run_in(
    registry: &Registry,
    session: &mut Session,
    request: impl FnOnce(&Session) -> Value,
) -> OperationOutcome {
    let mut manager = JobManager::default();
    let accepted = submit_in(&mut manager, registry, session, request).unwrap();
    wait_in(&manager, session, &accepted.run_id)
}

/// The error that refuses `request` before anything runs.
pub fn refused_in(
    registry: &Registry,
    session: &mut Session,
    request: impl FnOnce(&Session) -> Value,
) -> String {
    let mut manager = JobManager::default();
    match submit_in(&mut manager, registry, session, request) {
        Ok(accepted) => panic!("request was accepted: {accepted:#?}"),
        Err(error) => error.to_string(),
    }
}

/// Run `request` without a session, as `operations run` does for file inputs.
pub fn run(registry: &Registry, root: &Path, request: Value) -> OperationOutcome {
    let mut manager = JobManager::default();
    let accepted = manager
        .submit(
            registry,
            serde_json::from_value(request).unwrap(),
            SessionPolicy::new(root, true).unwrap(),
            "test",
        )
        .unwrap();
    wait(&manager, &accepted.run_id)
}

/// Wait for a run that needs no owner session.
pub fn wait(manager: &JobManager, id: &str) -> OperationOutcome {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let outcome = manager.get(id).unwrap();
        if outcome.state.terminal() {
            return outcome;
        }
        assert!(Instant::now() < deadline, "run {id} did not terminate");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn assert_completed(outcome: &OperationOutcome) {
    assert_eq!(outcome.state, RuntimeState::Completed, "{outcome:#?}");
}

/// The run's own folder: its evidence, publication and retained collection.
pub fn evidence(outcome: &OperationOutcome) -> PathBuf {
    PathBuf::from(outcome.evidence.as_ref().unwrap())
}

/// A member of the run's retained `artifacts` collection.
pub fn member(outcome: &OperationOutcome, name: &str) -> PathBuf {
    let path = evidence(outcome).join("artifacts").join(name);
    assert!(path.is_file(), "{} missing: {outcome:#?}", path.display());
    path
}

/// The first published artifact whose file name is `name`, wherever it lives.
pub fn artifact(outcome: &OperationOutcome, name: &str) -> PathBuf {
    outcome
        .artifacts
        .iter()
        .map(|artifact| PathBuf::from(&artifact.path))
        .find(|path| path.file_name().is_some_and(|file| file == name))
        .unwrap_or_else(|| panic!("no artifact {name}: {outcome:#?}"))
}

/// Sorted names in `root`, including `.basecraft` when present.
pub fn names(root: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Sorted names in `root` other than `.basecraft`.
pub fn visible(root: &Path) -> Vec<String> {
    let mut names = names(root);
    names.retain(|name| name != ".basecraft");
    names
}

/// The parameters of every `tool.run` the session recorded for `doc`.
pub fn tool_runs(session: &Session, doc: DocId) -> Vec<Value> {
    session
        .document_record(doc)
        .unwrap()
        .events
        .iter()
        .cloned()
        .filter_map(|event| match event.op {
            basecraft_protocol::Op::ToolRun { params, .. } => Some(params),
            _ => None,
        })
        .collect()
}

/// Every `tool.run` the session recorded for `doc`.
pub fn tool_run_events(session: &Session, doc: DocId) -> Vec<basecraft_protocol::LogEntry> {
    session
        .document_record(doc)
        .unwrap()
        .events
        .iter()
        .filter(|&event| matches!(event.op, basecraft_protocol::Op::ToolRun { .. }))
        .cloned()
        .collect()
}

/// The parameters an operation's descriptor declares.
pub fn declared(registry: &Registry, operation: &str) -> Vec<String> {
    registry
        .resolve(operation, None)
        .unwrap()
        .descriptor
        .parameters["properties"]
        .as_object()
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

/// `parameters` without the ones `operation` does not declare.
pub fn declared_only(registry: &Registry, operation: &str, parameters: &Value) -> Value {
    let declared = declared(registry, operation);
    let mut parameters = parameters.as_object().unwrap().clone();
    parameters.retain(|key, _| declared.contains(key));
    Value::Object(parameters)
}

pub fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// A stand-in for MAFFT `align` that copies its input and records the host
/// allocation it was given, for storage and evidence tests.
const COPY_ALIGNER: &str = r"
import json
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin('basecraft.mafft', 'test')
@plugin.operation('align')
def align(snapshot, params):
    allocation = params['analysis_artifacts']
    root = Path(allocation['metadata_directory'])
    assert root.is_absolute() and root.is_dir()
    assert Path(allocation['working_directory']).is_absolute()
    assert allocation['run_id'] == params['job_id']
    source, target = Path(params['input']), Path(params['output'])
    target.write_bytes(source.read_bytes())
    receipt = root / 'sdk-allocation.json'
    receipt.write_text(json.dumps(allocation))
    return {'ops': [], 'outputs': [str(target), str(receipt)]}
plugin.contract()
plugin.run()
";

/// A registry whose `basecraft.mafft/align@1` is the copying stand-in, kept
/// under `root`.
pub fn copy_aligner(root: &Path) -> Registry {
    registry(&fixture_package(root, "mafft", "align", COPY_ALIGNER))
}

/// Align `source` with the copying stand-in, as an open document of `session`,
/// exporting the result to `output`; the result verifies.
pub fn align_copy(
    registry: &Registry,
    session: &mut Session,
    source: &Path,
    output: &Path,
) -> OperationOutcome {
    let doc = session.open(source).unwrap();
    align_document(registry, session, doc, output)
}

/// Align the open document `doc` with the copying stand-in, as `align_copy`.
pub fn align_document(
    registry: &Registry,
    session: &mut Session,
    doc: DocId,
    output: &Path,
) -> OperationOutcome {
    let outcome = run_in(registry, session, |s| {
        json!({"operation":"basecraft.mafft/align@1","inputs":{"alignment":document(s, doc)},
            "parameters":{},"destinations":{"artifacts/alignment.afa":output}})
    });
    assert_completed(&outcome);
    assert_eq!(
        basecraft_ops::provenance::verify(output).unwrap(),
        Vec::<String>::new()
    );
    outcome
}

/// The analyses in a save conflict, without the reference ledger a registered
/// run keeps for each of its inputs; that ledger is listed beside its run.
pub fn analyses(
    conflict: &basecraft_ops::input_evidence::EvidenceConflict,
) -> Vec<&basecraft_ops::input_evidence::AffectedAnalysis> {
    conflict
        .analyses
        .iter()
        .filter(|analysis| read_json(&analysis.run.join("run.json"))["status"] != "referenced")
        .collect()
}

/// Resolve the verified original through its shared input owner. Tests must not
/// assume every backing ledger owns a separate physical preservation copy.
pub fn verified_input(run: &Path) -> PathBuf {
    let input = basecraft_ops::input_evidence::read_input(run).unwrap();
    basecraft_ops::input_evidence::verified_source(run, input.source.as_ref().unwrap()).unwrap()
}
