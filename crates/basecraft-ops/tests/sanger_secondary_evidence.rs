//! Reviewer regression for SCI-0015: exercise the real saved call-set envelope.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::sanger_review::{
    ReviewChoice, ReviewOp, SangerReviewSession, instrument_review,
};
use std::path::Path;

#[test]
fn secondary_evidence_survives_call_set_selection_save_and_replay() {
    crate::isolated_profile::isolate!();
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("trace.ab1");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &source,
    )
    .unwrap();
    let mut instrument = instrument_review(&source).unwrap();
    let instrument_id = instrument.draft_sha256.clone();
    let mut raw = serde_json::to_value(&instrument.draft).unwrap();
    raw["engine"] = "Tracy".into();
    raw["mode"] = "basecall".into();
    raw["engine_sha256"] = "a".repeat(64).into();
    let primary = raw["inputs"][0]["calls"]["sequence"].as_str().unwrap();
    let secondary: String = primary
        .chars()
        .map(|base| if base == 'A' { 'C' } else { 'A' })
        .collect();
    raw["inputs"][0]["calls"]["secondary_sequence"] = secondary.clone().into();
    let draft = serde_json::from_value(raw).unwrap();
    let mut computed = SangerReviewSession::from_draft(draft, &source).unwrap();
    let computed_id = computed.draft_sha256.clone();
    computed
        .apply(ReviewOp::Set {
            columns: vec![0],
            choice: ReviewChoice::Edit('C'),
        })
        .unwrap();
    instrument
        .apply(ReviewOp::Set {
            columns: vec![0],
            choice: ReviewChoice::Edit('G'),
        })
        .unwrap();
    instrument.retain_call_set(computed).unwrap();
    instrument.select_call_set(&computed_id).unwrap();
    let mut mask = instrument.mask.clone();
    mask.start = 1;
    instrument.apply(ReviewOp::Mask { settings: mask }).unwrap();
    let saved = tmp.path().join("review.json");
    instrument.save(&saved).unwrap();
    let mut reopened = SangerReviewSession::load(&saved).unwrap();
    assert_eq!(reopened.call_sets().len(), 2);
    assert_eq!(reopened.draft_sha256, computed_id);
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('C')));
    assert_eq!(reopened.mask.start, 1);
    let evidence = serde_json::to_value(reopened.working_draft().unwrap()).unwrap();
    assert_eq!(
        evidence["inputs"][0]["calls"]["secondary_sequence"],
        secondary
    );
    reopened.apply(ReviewOp::Undo).unwrap();
    assert_eq!(reopened.mask.start, 0);
    reopened.apply(ReviewOp::Redo).unwrap();
    assert_eq!(reopened.mask.start, 1);
    reopened.select_call_set(&instrument_id).unwrap();
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
    let evidence = serde_json::to_value(reopened.working_draft().unwrap()).unwrap();
    assert!(evidence["inputs"][0]["calls"]["secondary_sequence"].is_null());
    reopened.select_call_set(&computed_id).unwrap();
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('C')));
    let saved_again = tmp.path().join("review-again.json");
    reopened.save(&saved_again).unwrap();
    let reopened = SangerReviewSession::load(&saved_again).unwrap();
    let evidence = serde_json::to_value(reopened.working_draft().unwrap()).unwrap();
    assert_eq!(
        evidence["inputs"][0]["calls"]["secondary_sequence"],
        secondary
    );
}
