//! The single-trace caller service through the registry, jobs and retained
//! receipts. A stand-in caller package returns a recorded draft, so the host's
//! part is checked without Tracy; the live test needs the real program.
use basecraft_plugin_host::runner::{
    CompletedResultQuery, lookup_completed_result, verify_completed_result,
};
use basecraft_plugin_host::sanger_calls::SangerCaller;
use std::path::Path;

/// A caller that answers `engine` and copies the recorded draft it was given,
/// after checking the settings the host sent.
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
    assert parameters == {"engine_sha256": ENGINE, "minimum_identity": 0.8,
                          "minimum_overlap": 50, "signal_threshold": 20}, parameters
    recorded = Path(__file__).with_name("recorded.json").read_bytes()
    Path(context.output("artifacts", "evidence.json")).write_bytes(recorded)
    return {"message": "recorded draft"}

@plugin.operation("calls")
def calls(parameters, context):
    raise ValueError("unused")

@plugin.operation("primers")
def primers(parameters, context):
    raise ValueError("unused")

plugin.run()
"#;

fn fixture_ab1() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/compact/sanger-review/forward.ab1")
}

#[test]
fn a_called_draft_is_the_recorded_draft_with_its_source_and_settings() {
    // The service finds callers in the bundled plugin folder, which the
    // environment names; a child process gets its own environment.
    let Some(bundle) = std::env::var_os("BASECRAFT_SANGER_CALLER_BUNDLE") else {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("bundle");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "a_called_draft_is_the_recorded_draft_with_its_source_and_settings",
            ])
            .env("BASECRAFT_SANGER_CALLER_BUNDLE", &bundle)
            .env("BASECRAFT_PLUGINS", &bundle)
            .status()
            .unwrap();
        assert!(status.success());
        return;
    };
    let bundle = std::path::PathBuf::from(bundle);
    let temp = tempfile::tempdir().unwrap();
    let package = bundle.join("tracy-sanger");
    std::fs::create_dir_all(&package).unwrap();
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/tracy-sanger");
    for file in ["plugin.toml", "operations.json"] {
        std::fs::copy(plugins.join(file), package.join(file)).unwrap();
    }
    std::fs::write(package.join("tracy_plugin.py"), STAND_IN).unwrap();
    let source = temp.path().join("read.ab1");
    std::fs::copy(fixture_ab1(), &source).unwrap();
    let mut recorded = basecraft_ops::sanger_review::instrument_review(&source)
        .unwrap()
        .draft;
    recorded.engine = "Tracy".into();
    recorded.mode = "basecall".into();
    recorded.engine_sha256 = "e".repeat(64);
    recorded.signal_threshold = Some(20);
    recorded.minimum_overlap = 50;
    recorded.minimum_identity = 0.8;
    std::fs::write(
        package.join("recorded.json"),
        serde_json::to_vec(&recorded).unwrap(),
    )
    .unwrap();

    let sha256 = basecraft_io::sha256_file(&source).unwrap();
    let mut caller = SangerCaller::default();
    assert!(caller.start(&source, &sha256, 20).unwrap().is_none());
    let started = std::time::Instant::now();
    let called = loop {
        if let Some(called) = caller.poll().unwrap() {
            break called;
        }
        assert!(started.elapsed().as_secs() < 60, "caller timeout");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };

    // The host adds the source path and one line naming what ran; the
    // settings line is the one earlier builds wrote for the same run.
    let mut expected = recorded.clone();
    expected.inputs[0].path.clone_from(&source);
    let runtime: basecraft_protocol::operation::OperationOutcome =
        serde_json::from_slice(&std::fs::read(called.run.join("runtime.json")).unwrap()).unwrap();
    assert_eq!(
        runtime.effective_parameters.to_string(),
        format!(
            r#"{{"engine_sha256":"{}","minimum_identity":0.8,"minimum_overlap":50,"signal_threshold":20}}"#,
            "e".repeat(64)
        )
    );
    expected.limitations.push(format!(
        "Caller package {}; contract {}; effective settings {}",
        runtime.pins.package_digest, runtime.pins.descriptor_digest, runtime.effective_parameters
    ));
    assert_eq!(
        serde_json::to_vec(&called.draft).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    // The same read and settings reuse the retained result without a new run.
    let again = caller.start(&source, &sha256, 20).unwrap().unwrap();
    assert_eq!(again.run, called.run);
    assert_eq!(
        serde_json::to_vec(&again.draft).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
}

#[test]
#[ignore = "requires BASECRAFT_SANGER_FIXTURE, public plugin Python and Tracy"]
fn actual_open_call_review_reopen_cache_cancel_and_stale_source() {
    use basecraft_ops::sanger_review::{ReviewChoice, ReviewOp, SangerReview};
    let fixture = std::env::var_os("BASECRAFT_SANGER_FIXTURE").expect("public AB1 fixture");
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.ab1");
    std::fs::copy(fixture, &source).unwrap();
    let original = std::fs::read(&source).unwrap();
    let mut review = basecraft_ops::sanger_review::instrument_review(&source).unwrap();
    let instrument_id = review.draft_sha256.clone();
    let sha256 = review.draft.inputs[0].sha256.clone();
    let mut caller = SangerCaller::default();
    assert!(caller.start(&source, &sha256, 20).unwrap().is_none());
    // Review remains editable through Session while the shared job runs.
    let mut session = basecraft_ops::Session::recoverable(tmp.path()).unwrap();
    let id = session.create_sanger_review(&review).unwrap();
    session
        .apply(
            id,
            basecraft_protocol::Op::SangerReview {
                call_set: instrument_id.clone(),
                edit: Box::new(ReviewOp::Set {
                    columns: vec![0],
                    choice: ReviewChoice::Edit('G'),
                }),
            },
        )
        .unwrap();
    let started = std::time::Instant::now();
    let called = loop {
        if let Some(called) = caller.poll().unwrap() {
            break called;
        }
        assert!(started.elapsed().as_secs() < 120, "caller timeout");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    // Shared host verification, not Sanger, owns common receipt/input/artifact rules.
    let receipt: basecraft_protocol::operation::OperationOutcome = serde_json::from_slice(
        &basecraft_ops::records::read_record(&called.run.join("runtime.json")).unwrap(),
    )
    .unwrap();
    let execution: basecraft_protocol::records::RunRecord = serde_json::from_slice(
        &basecraft_ops::records::read_record(&called.run.join("execution.json")).unwrap(),
    )
    .unwrap();
    let mut query = CompletedResultQuery {
        operation: receipt.operation.clone(),
        pins: receipt.pins.clone(),
        effective_parameters: receipt.effective_parameters.clone(),
        inputs: execution.inputs,
    };
    assert!(verify_completed_result(&receipt, &query).is_ok());
    query.inputs.get_mut("reads").unwrap()[0].submitted_hash = "b3:changed".into();
    assert!(verify_completed_result(&receipt, &query).is_err());
    query.inputs.get_mut("reads").unwrap()[0].submitted_hash =
        basecraft_io::hash_file(&source).unwrap();
    query.pins.package_digest = "changed producer".into();
    assert!(verify_completed_result(&receipt, &query).is_err());
    query.pins = receipt.pins.clone();
    let mut future = receipt.clone();
    future.schema = 99;
    assert!(verify_completed_result(&future, &query).is_err());
    let fake = basecraft_ops::analysis_store::ArtifactRun::create(&source, "0000-future").unwrap();
    basecraft_ops::input_evidence::write_json(&fake.artifact("runtime.json").unwrap(), &future)
        .unwrap();
    let corrupt =
        basecraft_ops::analysis_store::ArtifactRun::create(&source, "0001-corrupt").unwrap();
    std::fs::write(
        corrupt.artifact("runtime.json").unwrap(),
        b"corrupt receipt",
    )
    .unwrap();
    let found = lookup_completed_result(&source, &query);
    assert!(found.result.is_some());
    assert_eq!(found.diagnostics.len(), 2);
    let artifact_path = Path::new(&receipt.artifacts[0].path);
    let retained_bytes = std::fs::read(artifact_path).unwrap();
    std::fs::write(artifact_path, b"damaged retained output").unwrap();
    assert!(verify_completed_result(&receipt, &query).is_err());
    std::fs::write(artifact_path, retained_bytes).unwrap();
    let mut computed = SangerReview::from_draft(called.draft, &source).unwrap();
    computed.attach_operation_run(&called.run).unwrap();
    let computed_id = computed.draft_sha256.clone();
    review = session.sanger_review(id).unwrap().clone();
    review.retain_call_set(computed).unwrap();
    session.update_sanger_review(id, &review).unwrap();
    session
        .apply(
            id,
            basecraft_protocol::Op::SangerReview {
                call_set: computed_id.clone(),
                edit: Box::new(ReviewOp::Set {
                    columns: vec![0],
                    choice: ReviewChoice::Edit('C'),
                }),
            },
        )
        .unwrap();
    assert_eq!(
        review.draft_sha256, instrument_id,
        "completion must not switch calls"
    );
    assert_eq!(review.decision(0), Some(&ReviewChoice::Edit('G')));
    assert!(
        caller.start(&source, &sha256, 20).unwrap().is_some(),
        "reuse completed receipt"
    );
    session
        .apply(
            id,
            basecraft_protocol::Op::SangerSelect {
                call_set: computed_id,
            },
        )
        .unwrap();
    let saved = tmp.path().join("saved.json");
    session.save(id, Some(&saved)).unwrap();
    let mut reopened = basecraft_ops::Session::recoverable(tmp.path()).unwrap();
    let id = reopened.open(&saved).unwrap();
    assert_eq!(
        reopened.sanger_review(id).unwrap().decision(0),
        Some(&ReviewChoice::Edit('C'))
    );
    reopened
        .apply(
            id,
            basecraft_protocol::Op::SangerSelect {
                call_set: instrument_id,
            },
        )
        .unwrap();
    assert_eq!(
        reopened.sanger_review(id).unwrap().decision(0),
        Some(&ReviewChoice::Edit('G'))
    );
    assert!(caller.start(&source, &sha256, 19).unwrap().is_none());
    caller.cancel().unwrap();
    assert!(
        caller.poll().unwrap().is_none(),
        "late completion must be detached after cancel"
    );
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert!(caller.start(&source, &sha256, 18).unwrap().is_none());
    std::fs::write(&source, "source replaced while caller was running").unwrap();
    let started = std::time::Instant::now();
    loop {
        match caller.poll() {
            Err(_) => break,
            Ok(Some(_)) => panic!("stale completion must never be reusable"),
            Ok(None) => {}
        }
        assert!(started.elapsed().as_secs() < 120, "stale caller timeout");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::fs::write(&source, "different bytes at the same path").unwrap();
    assert!(caller.start(&source, &sha256, 20).is_err());
    assert_eq!(
        reopened.sanger_review(id).unwrap().decision(0),
        Some(&ReviewChoice::Edit('G'))
    );
}
