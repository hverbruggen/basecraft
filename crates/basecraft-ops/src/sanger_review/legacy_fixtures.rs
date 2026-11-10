//! Compatibility oracles captured before the protocol extraction. No production writer changes.
use super::*;
use crate::sanger_review::{MaskSettings, ReviewChoice, ReviewOp};
use basecraft_docs::sanger::{ReviewReference, ReviewSettings};
use serde_json::json;

fn evaluate(version: u32) -> ReviewOp {
    ReviewOp::Evaluate {
        version,
        settings: ReviewSettings {
            strong_score: 40,
            ..ReviewSettings::default()
        },
        calls: vec![Some('A'), Some('C'), None, Some('T')],
        reference: Some(ReviewReference {
            source_path: "reference.afa".into(),
            source_sha256: basecraft_io::sha256_bytes(b">one\nACGT\n"),
            counts: vec![
                Some([1, 0, 0, 0]),
                Some([0, 1, 0, 0]),
                None,
                Some([0, 0, 0, 1]),
            ],
        }),
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/sanger-legacy")
}

/// Run only in an empty, isolated capture directory, before moving the types.
/// The recipe mounts it at /fixture; no host path enters opaque draft bytes.
#[test]
#[ignore = "explicit baseline capture only; never regenerate compatibility oracles during tests"]
fn capture_legacy_writer() {
    assert_eq!(
        std::env::var("BASECRAFT_CAPTURE").as_deref(),
        Ok("legacy-writer")
    );
    std::env::set_current_dir("/fixture").unwrap();
    assert!(!Path::new("single.json").exists());
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        "trace.ab1",
    )
    .unwrap();
    std::fs::write("reference.afa", b">one\nACGT\n").unwrap();
    let columns: Vec<_> = "ACGT"
        .chars()
        .enumerate()
        .map(|(i, b)| {
            json!({
                "column":i+1,"consensus":b,"support":[{"read":1,"base":b,"kind":"call"}],
                "conflict":false,"review":true
            })
        })
        .collect();
    let mut draft = json!({
        "status":"draft_requires_review","engine":"Tracy","version_output":"synthetic legacy fixture",
        "engine_sha256":"a".repeat(64),"mode":"basecall",
        "inputs":[{"read":1,"path":"trace.ab1","sha256":basecraft_io::sha256_file(Path::new("trace.ab1")).unwrap(),"orientation":"forward"}],
        "consensus":"ACGT","consensus_length":4,"minimum_overlap":1,"minimum_identity":0.8,
        "overlaps":[],"aligned_columns":4,"conflict_columns":0,"columns":columns,
        "opaque_extension":{"keep":"original whitespace and this unknown field"}
    });
    // Deliberately retain pretty formatting and a trailing newline as opaque input.
    std::fs::write(
        "draft-a.json",
        format!("{}\n", serde_json::to_string_pretty(&draft).unwrap()),
    )
    .unwrap();
    draft["version_output"] = json!("alternative synthetic caller version");
    std::fs::write(
        "draft-b.json",
        format!("{}\n", serde_json::to_string_pretty(&draft).unwrap()),
    )
    .unwrap();
    let mut a = SangerReviewSession::new(Path::new("draft-a.json")).unwrap();
    a.apply(ReviewOp::AutoAccept {
        columns: vec![0, 1],
    })
    .unwrap();
    a.apply(evaluate(1)).unwrap();
    a.apply(ReviewOp::Undo).unwrap();
    assert_eq!(
        a.decisions,
        vec![
            ReviewChoice::Accept,
            ReviewChoice::Accept,
            ReviewChoice::Pending,
            ReviewChoice::Pending
        ]
    );
    a.apply(ReviewOp::Redo).unwrap();
    for (column, choice) in [
        (0, ReviewChoice::Edit('G')),
        (1, ReviewChoice::Reject),
        (2, ReviewChoice::Pending),
        (3, ReviewChoice::Accept),
    ] {
        a.apply(ReviewOp::Set {
            columns: vec![column],
            choice,
        })
        .unwrap();
    }
    a.apply(ReviewOp::Mask {
        settings: MaskSettings {
            start: 0,
            end: 1,
            forward: "AC".into(),
            reverse: "GT".into(),
            minimum_overlap: 8,
            error_percent: 10,
            evidence: "synthetic primer evidence".into(),
        },
    })
    .unwrap();
    a.apply(ReviewOp::Undo).unwrap();
    a.apply(ReviewOp::Redo).unwrap();
    a.export_consensus(Path::new("/fixture/consensus.fa"), "synthetic")
        .unwrap();
    a.apply(evaluate(2)).unwrap();
    a.save(Path::new("/fixture/single.json")).unwrap();
    let mut b = SangerReviewSession::new(Path::new("draft-b.json")).unwrap();
    b.apply(evaluate(2)).unwrap();
    assert!(b.apply(ReviewOp::Undo).is_err());
    b.apply(ReviewOp::Set {
        columns: vec![1],
        choice: ReviewChoice::Edit('T'),
    })
    .unwrap();
    b.apply(ReviewOp::Undo).unwrap();
    b.apply(ReviewOp::Redo).unwrap();
    let selected = b.draft_sha256.clone();
    a.retain_call_set(b).unwrap();
    a.select_call_set(&selected).unwrap();
    a.save(Path::new("/fixture/multi.json")).unwrap();
    let checkpoint = a.recovery_path().unwrap();
    std::fs::write(
        "capture-index.json",
        serde_json::to_vec_pretty(&json!({
            "checkpoint": checkpoint.strip_prefix("/fixture").unwrap(),
            "selected":selected,
            "single_sequence":"G", "multi_sequence":null,
            "writer_base":"4044160b17a8de501fe7518faa3e5d67ca03dd8c"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn evaluate_initialization_depends_on_first_event_not_first_evaluate() {
    let root = fixture_root();
    let fresh = || SangerReviewSession::new(&root.join("draft-a.json")).unwrap();
    let mut first = fresh();
    first.apply(evaluate(2)).unwrap();
    assert!(first.apply(ReviewOp::Undo).is_err());
    assert_eq!(first.events.len(), 1);
    let mut later = fresh();
    later
        .apply(ReviewOp::AutoAccept {
            columns: vec![0, 1],
        })
        .unwrap();
    let before = (
        later.decisions.clone(),
        later.manual.clone(),
        later.settings.clone(),
        later.reference.clone(),
    );
    later.apply(evaluate(1)).unwrap();
    later.apply(ReviewOp::Undo).unwrap();
    assert_eq!(
        (
            later.decisions.clone(),
            later.manual.clone(),
            later.settings.clone(),
            later.reference.clone()
        ),
        before
    );
    later.apply(ReviewOp::Redo).unwrap();
    assert_eq!(later.reference, first.reference);
    assert_eq!(later.decisions, first.decisions);
}

fn checkpoint_path(root: &Path) -> PathBuf {
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("capture-index.json")).unwrap()).unwrap();
    root.join(index["checkpoint"].as_str().unwrap())
}

#[test]
fn frozen_writer_bytes_drafts_support_and_selected_history() {
    let root = fixture_root().canonicalize().unwrap();
    // A child selects the captured job layout as application state without
    // process-global environment mutation or touching the developer's profile.
    if std::env::var_os("BASECRAFT_LEGACY_CHECKPOINT_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sanger_review::storage::legacy_fixtures::frozen_writer_bytes_drafts_support_and_selected_history", "--nocapture"])
            .env("BASECRAFT_LEGACY_CHECKPOINT_CHILD", "1")
            .env("BASECRAFT_PROFILE_DIR", root.join("profile"))
            .env("BASECRAFT_STATE_DIR", root.join("profile/state"))
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let digests: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&std::fs::read(root.join("sha256.json")).unwrap()).unwrap();
    for (relative, digest) in digests {
        assert_eq!(
            basecraft_io::sha256_file(&root.join(&relative)).unwrap(),
            digest,
            "{relative}"
        );
    }
    for path in [
        root.join("single.json"),
        root.join("multi.json"),
        checkpoint_path(&root),
    ] {
        let original = std::fs::read(&path).unwrap();
        let mut review = SangerReviewSession::load(&path).unwrap();
        // Reuse the loaded run IDs and the exact logical destination. Do not save:
        // save_durable would allocate another ID and create a different record.
        let mut bytes = serde_json::to_vec_pretty(&review.stored_at(&path).unwrap()).unwrap();
        if original.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            String::from_utf8(original).unwrap(),
            "{}",
            path.display()
        );
        for set in std::iter::once(&review).chain(review.retained.iter()) {
            assert_eq!(
                set.draft_json.as_bytes(),
                std::fs::read(root.join(&set.draft_path)).unwrap()
            );
            assert_eq!(
                set.draft_sha256,
                basecraft_io::sha256_bytes(set.draft_json.as_bytes())
            );
            assert!(set.draft_json.contains("opaque_extension"));
            for source in &set.evidence.sources {
                assert_eq!(
                    basecraft_io::sha256_file(&source.path).unwrap(),
                    source.sha256
                );
                assert!(source.ledger.as_ref().unwrap().join("input.json").is_file());
            }
            for run in &set.evidence.runs {
                assert!(run.join("manifest.json").is_file());
                assert!(run.join("draft.json").is_file());
            }
        }
        if path == checkpoint_path(&root) {
            assert_eq!(review.recovery_path(), Some(path.as_path()));
            let manifest: serde_json::Value = serde_json::from_slice(
                &std::fs::read(path.with_file_name("manifest.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["operation"], "Sanger review recovery");
            assert_eq!(manifest["status"], "review_requires_attention");
            assert_eq!(manifest["schema_version"], 1);
            assert!(manifest["owner_instance"].as_str().unwrap().contains(':'));
            assert!(manifest["started"].as_str().unwrap().ends_with('Z'));
        }
        // Never let a fixture test write a recovery checkpoint.
        review.recovery = None;
        if path.file_name().unwrap() == "single.json" {
            assert!(review.retained.is_empty());
            assert_eq!(review.resolved_sequence().as_deref(), Some("G"));
            assert_eq!(review.manual, vec![true; 4]);
            assert_eq!(
                review.decisions,
                vec![
                    ReviewChoice::Edit('G'),
                    ReviewChoice::Reject,
                    ReviewChoice::Pending,
                    ReviewChoice::Accept
                ]
            );
            assert!(
                review
                    .events
                    .iter()
                    .any(|event| matches!(event, ReviewOp::Export { .. }))
            );
            assert!(
                review
                    .events
                    .iter()
                    .any(|event| matches!(event, ReviewOp::Evaluate { version: 1, .. }))
            );
            assert!(
                review
                    .events
                    .iter()
                    .any(|event| matches!(event, ReviewOp::Evaluate { version: 2, .. }))
            );
        } else {
            assert_eq!(review.draft_path, Path::new("draft-b.json"));
            assert_eq!(review.retained.len(), 1);
            assert_eq!(review.manual, vec![false, true, false, false]);
            assert_eq!(
                review.decisions,
                vec![
                    ReviewChoice::Auto('A'),
                    ReviewChoice::Edit('T'),
                    ReviewChoice::Pending,
                    ReviewChoice::Auto('T')
                ]
            );
            review.apply(ReviewOp::Undo).unwrap();
            assert_eq!(review.decision(1), Some(&ReviewChoice::Auto('C')));
            review.apply(ReviewOp::Redo).unwrap();
            let inactive = review.retained[0].draft_sha256.clone();
            review.select_call_set(&inactive).unwrap();
            assert_eq!(review.resolved_sequence().as_deref(), Some("G"));
            assert_eq!(review.manual, vec![true; 4]);
            review.apply(ReviewOp::Undo).unwrap(); // Evaluate after manual edits
            review.apply(ReviewOp::Undo).unwrap(); // Mask (export is not an undo entry)
            assert_eq!(review.mask.end, 4);
            review.apply(ReviewOp::Redo).unwrap();
            assert_eq!(review.mask.end, 1);
        }
    }
}

#[test]
fn legacy_extensions_and_invalid_records_keep_existing_contract() {
    let root = fixture_root().canonicalize().unwrap();
    let path = root.join("single.json");
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let decode = |value: &serde_json::Value| {
        SangerReviewSession::decode(&serde_json::to_vec(value).unwrap(), &path)
    };
    let mut value = original.clone();
    value["outer_extension"] = json!({"retained":true});
    value["evidence"]["evidence_extension"] = json!(17);
    value["evidence"]["sources"][0]["source_extension"] = json!("retained");
    // Support is optional in these fixtures; characterize its flatten map directly.
    let support: Support = serde_json::from_value(
        json!({"path":"placement.json","hash":"abc","support_extension":true}),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(support).unwrap()["support_extension"],
        true
    );
    let loaded = decode(&value).unwrap();
    assert_eq!(loaded.stored_at(&path).unwrap(), value);
    for event in value["events"].as_array_mut().unwrap() {
        event["ignored_event_extension"] = json!(true);
        if let Some(settings) = event.get_mut("settings") {
            settings["ignored_settings_extension"] = json!(true);
        }
        if event["reference"].is_object() {
            event["reference"]["ignored_reference_extension"] = json!(true);
        }
        if event["choice"].is_object() {
            event["choice"]["ignored_choice_extension"] = json!(true);
        }
    }
    let rewritten = decode(&value).unwrap().stored_at(&path).unwrap();
    assert!(
        !serde_json::to_string(&rewritten)
            .unwrap()
            .contains("ignored_")
    );
    assert_eq!(rewritten["events"], original["events"]);
    let mut multi: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("multi.json")).unwrap()).unwrap();
    multi["outer_extension"] = json!(true);
    assert!(decode(&multi).is_err());
    let mut bad = original.clone();
    bad["version"] = json!(3);
    assert_eq!(
        decode(&bad).err().unwrap(),
        "Unsupported Sanger review version"
    );
    bad = original.clone();
    bad["draft_sha256"] = json!("0".repeat(64));
    assert_eq!(
        decode(&bad).err().unwrap(),
        "Embedded source draft changed since review"
    );
    bad = original.clone();
    bad["events"][0]["op"] = json!("future_op");
    assert!(decode(&bad).is_err());
    bad = original;
    bad["events"][1]["version"] = json!(3);
    assert_eq!(
        decode(&bad).err().unwrap(),
        "Invalid automatic review evidence"
    );
}
