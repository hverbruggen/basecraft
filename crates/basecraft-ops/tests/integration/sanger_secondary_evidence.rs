//! Reviewer regression for SCI-0015: exercise the real saved call-set envelope.
use basecraft_ops::Session;
use basecraft_ops::sanger_review::{ReviewChoice, ReviewOp, SangerReview, instrument_review};
use basecraft_protocol::Op;
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
    let computed = SangerReview::from_draft(draft, &source).unwrap();
    let computed_id = computed.draft_sha256.clone();
    instrument.retain_call_set(computed).unwrap();
    let mut session = Session::recoverable(tmp.path()).unwrap();
    let id = session.create_sanger_review(&instrument).unwrap();
    session
        .apply(
            id,
            Op::SangerReview {
                call_set: computed_id.clone(),
                edit: Box::new(ReviewOp::Set {
                    columns: vec![0],
                    choice: ReviewChoice::Edit('C'),
                }),
            },
        )
        .unwrap();
    session
        .apply(
            id,
            Op::SangerReview {
                call_set: instrument_id.clone(),
                edit: Box::new(ReviewOp::Set {
                    columns: vec![0],
                    choice: ReviewChoice::Edit('G'),
                }),
            },
        )
        .unwrap();
    session
        .apply(
            id,
            Op::SangerSelect {
                call_set: computed_id.clone(),
            },
        )
        .unwrap();
    let mut mask = instrument.mask.clone();
    mask.start = 1;
    session
        .apply(
            id,
            Op::SangerReview {
                call_set: computed_id.clone(),
                edit: Box::new(ReviewOp::Mask { settings: mask }),
            },
        )
        .unwrap();
    let saved = tmp.path().join("review.json");
    session.save(id, Some(&saved)).unwrap();
    let mut session = Session::recoverable(tmp.path()).unwrap();
    let id = session.open(&saved).unwrap();
    let reopened = session.sanger_review(id).unwrap();
    assert_eq!(reopened.call_sets().len(), 2);
    assert_eq!(reopened.draft_sha256, computed_id);
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('C')));
    assert_eq!(reopened.mask.start, 1);
    let evidence = serde_json::to_value(reopened.working_draft().unwrap()).unwrap();
    assert_eq!(
        evidence["inputs"][0]["calls"]["secondary_sequence"],
        secondary
    );
    assert_eq!(reopened.mask.start, 1);
    session
        .apply(
            id,
            Op::SangerSelect {
                call_set: instrument_id.clone(),
            },
        )
        .unwrap();
    let reopened = session.sanger_review(id).unwrap();
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('G')));
    let evidence = serde_json::to_value(reopened.working_draft().unwrap()).unwrap();
    assert!(evidence["inputs"][0]["calls"]["secondary_sequence"].is_null());
    session
        .apply(
            id,
            Op::SangerSelect {
                call_set: computed_id.clone(),
            },
        )
        .unwrap();
    let reopened = session.sanger_review(id).unwrap();
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('C')));
    let saved_again = tmp.path().join("review-again.json");
    session.save(id, Some(&saved_again)).unwrap();
    let reopened = SangerReview::load(&saved_again).unwrap();
    let evidence = serde_json::to_value(reopened.working_draft().unwrap()).unwrap();
    assert_eq!(
        evidence["inputs"][0]["calls"]["secondary_sequence"],
        secondary
    );
}
