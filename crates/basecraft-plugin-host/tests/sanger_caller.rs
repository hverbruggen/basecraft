//! The single-trace caller service through the registry, jobs and retained
//! receipts. A stand-in caller returns a recorded draft, so the host's part
//! runs without Tracy: the draft it hands back, reuse, receipt checks,
//! cancellation and a source that changes under it.
use basecraft_plugin_host::runner::{
    CompletedResultQuery, lookup_completed_result, verify_completed_result,
};
use basecraft_plugin_host::sanger_calls::{CalledDraft, SangerCaller};
use std::path::{Path, PathBuf};

/// Answers `engine` and returns the `recorded.json` beside its bundle folder,
/// outside the package, so changing it does not change the package.
const STAND_IN: &str = r#"
import json
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin("basecraft.tracy-sanger", "0.2.0")
ENGINE = "e" * 64

@plugin.query("engine")
def engine():
    return {"name": "Tracy", "sha256": ENGINE, "problem": None}

@plugin.operation("consensus")
def consensus(parameters, context):
    assert parameters["engine_sha256"] == ENGINE and parameters["minimum_overlap"] == 50, parameters
    recorded = (Path(__file__).resolve().parents[2] / "recorded.json").read_bytes()
    if parameters["signal_threshold"] != 20:
        draft = json.loads(recorded)
        draft["signal_threshold"] = parameters["signal_threshold"]
        recorded = json.dumps(draft).encode()
    Path(context.output("artifacts", "evidence.json")).write_bytes(recorded)
    return {"message": "recorded draft"}

for name in ("calls", "primers"):
    plugin.operation(name)(lambda parameters, context: {})
plugin.run()
"#;

/// A caller that hangs is cancelled after 60 s and fails the test.
fn called(caller: &mut SangerCaller) -> Result<CalledDraft, String> {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watchdog = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(60));
        watchdog.store(true, std::sync::atomic::Ordering::Release);
    });
    let started = std::time::Instant::now();
    let called = caller.wait(&cancel);
    assert!(started.elapsed().as_secs() < 60, "caller timeout");
    called
}

#[test]
fn the_caller_service_returns_checks_reuses_and_refuses_drafts() {
    // The service finds callers in the bundled plugin folder, which the
    // environment names; a child process gets its own environment.
    let Some(bundle) = std::env::var_os("BASECRAFT_SANGER_CALLER_BUNDLE") else {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("bundle");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "the_caller_service_returns_checks_reuses_and_refuses_drafts",
            ])
            .env("BASECRAFT_SANGER_CALLER_BUNDLE", &bundle)
            .env("BASECRAFT_PLUGINS", &bundle)
            .status()
            .unwrap();
        assert!(status.success());
        return;
    };
    let bundle = PathBuf::from(bundle);
    let package = bundle.join("tracy-sanger");
    std::fs::create_dir_all(&package).unwrap();
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/tracy-sanger");
    for file in ["plugin.toml", "operations.json"] {
        std::fs::copy(plugins.join(file), package.join(file)).unwrap();
    }
    std::fs::write(package.join("tracy_plugin.py"), STAND_IN).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("read.ab1");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &source,
    )
    .unwrap();
    let mut recorded = basecraft_ops::sanger_review::instrument_review(&source)
        .unwrap()
        .draft;
    recorded.engine = "Tracy".into();
    recorded.mode = "basecall".into();
    recorded.engine_sha256 = "e".repeat(64);
    recorded.signal_threshold = Some(20);
    recorded.minimum_overlap = 50;
    recorded.minimum_identity = 0.8;
    let record = |draft: &basecraft_docs::sanger::SangerDraft| {
        std::fs::write(
            bundle.parent().unwrap().join("recorded.json"),
            serde_json::to_vec(draft).unwrap(),
        )
        .unwrap();
    };
    record(&recorded);
    let sha256 = basecraft_io::sha256_file(&source).unwrap();
    let mut caller = SangerCaller::default();
    assert!(caller.start(&source, &sha256, 20).unwrap().is_none());
    let first = called(&mut caller).unwrap();

    // The draft is the recorded one with its source path and one line naming
    // what ran; the settings are those earlier builds sent for this run.
    let receipt: basecraft_protocol::operation::OperationOutcome =
        serde_json::from_slice(&std::fs::read(first.run.join("runtime.json")).unwrap()).unwrap();
    assert_eq!(
        receipt.effective_parameters.to_string(),
        format!(
            r#"{{"engine_sha256":"{}","minimum_identity":0.8,"minimum_overlap":50,"signal_threshold":20}}"#,
            "e".repeat(64)
        )
    );
    let mut expected = recorded.clone();
    expected.inputs[0].path.clone_from(&source);
    expected.limitations.push(format!(
        "Caller package {}; contract {}; effective settings {}",
        receipt.pins.package_digest, receipt.pins.descriptor_digest, receipt.effective_parameters
    ));
    let bytes = |draft: &basecraft_docs::sanger::SangerDraft| serde_json::to_vec(draft).unwrap();
    assert_eq!(bytes(&first.draft), bytes(&expected));

    // The same read and settings reuse the retained result.
    let again = caller.start(&source, &sha256, 20).unwrap().unwrap();
    assert_eq!(
        (again.run.clone(), bytes(&again.draft)),
        (first.run.clone(), bytes(&expected))
    );

    // Shared receipt verification refuses another input, producer or schema,
    // and a lookup reports unreadable receipts beside the good one.
    let execution: basecraft_protocol::records::RunRecord =
        serde_json::from_slice(&std::fs::read(first.run.join("execution.json")).unwrap()).unwrap();
    let mut query = CompletedResultQuery {
        operation: receipt.operation.clone(),
        pins: receipt.pins.clone(),
        effective_parameters: receipt.effective_parameters.clone(),
        inputs: execution.inputs,
    };
    assert!(verify_completed_result(&receipt, &query).is_ok());
    let reads = query.inputs.keys().next().unwrap().clone();
    query.inputs.get_mut(&reads).unwrap()[0].submitted_hash = "b3:changed".into();
    assert!(verify_completed_result(&receipt, &query).is_err());
    query.inputs.get_mut(&reads).unwrap()[0].submitted_hash =
        basecraft_io::hash_file(&source).unwrap();
    query.pins.package_digest = "changed producer".into();
    assert!(verify_completed_result(&receipt, &query).is_err());
    query.pins = receipt.pins.clone();
    let mut future = receipt.clone();
    future.schema = 99;
    assert!(verify_completed_result(&future, &query).is_err());
    let fake = basecraft_ops::records::ArtifactRun::create(&source, "0000-future").unwrap();
    basecraft_ops::records::write_json(&fake.artifact("runtime.json").unwrap(), &future).unwrap();
    let corrupt = basecraft_ops::records::ArtifactRun::create(&source, "0001-corrupt").unwrap();
    std::fs::write(
        corrupt.artifact("runtime.json").unwrap(),
        b"corrupt receipt",
    )
    .unwrap();
    let found = lookup_completed_result(&source, &query);
    assert!(found.result.is_some());
    assert_eq!(found.diagnostics.len(), 2);
    let artifact = Path::new(&receipt.artifacts[0].path);
    let retained = std::fs::read(artifact).unwrap();
    std::fs::write(artifact, b"damaged retained output").unwrap();
    assert!(verify_completed_result(&receipt, &query).is_err());
    std::fs::write(artifact, retained).unwrap();

    // Calls that do not fit the trace are refused.
    let mut misplaced = recorded.clone();
    *misplaced.inputs[0]
        .calls
        .as_mut()
        .unwrap()
        .positions
        .last_mut()
        .unwrap() = u32::MAX;
    record(&misplaced);
    assert!(caller.start(&source, &sha256, 17).unwrap().is_none());
    assert!(
        called(&mut caller)
            .err()
            .unwrap()
            .contains("do not fit the original trace")
    );
    record(&recorded);

    // A cancelled run is detached; a source replaced mid-run gives no draft,
    // and a changed source is refused before a run starts.
    assert!(caller.start(&source, &sha256, 19).unwrap().is_none());
    caller.cancel().unwrap();
    assert!(called(&mut caller).is_err());
    assert!(caller.start(&source, &sha256, 18).unwrap().is_none());
    std::fs::write(&source, "source replaced while the caller ran").unwrap();
    assert!(called(&mut caller).is_err());
    assert!(caller.start(&source, &sha256, 20).is_err());
}
