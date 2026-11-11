//! Compatibility oracles captured before the protocol extraction. No production writer changes.
use super::*;
use crate::sanger_review::{ReviewChoice, ReviewOp};
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

#[test]
fn evaluate_initialization_depends_on_first_event_not_first_evaluate() {
    let root = fixture_root();
    let fresh = || SangerReview::new(&root.join("draft-a.json")).unwrap();
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
        let mut review = SangerReview::load(&path).unwrap();
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
        SangerReview::decode(&serde_json::to_vec(value).unwrap(), &path)
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
