//! Review decisions, structural continuations and recovery through the public Session.
use basecraft_ops::{Session, replay, sanger_review::SangerReview};
use basecraft_protocol::{
    Op,
    sanger::{ReviewChoice, ReviewOp},
};
use std::path::Path;
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

fn fixture() -> SangerReview {
    SangerReview::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/sanger-legacy/multi.json"),
    )
    .unwrap()
}
fn edit(session: &mut Session, id: basecraft_ops::DocId, base: char) {
    let call_set = session.sanger_review(id).unwrap().draft_sha256.clone();
    session
        .apply(
            id,
            Op::SangerReview {
                call_set,
                edit: Box::new(ReviewOp::Set {
                    columns: vec![0],
                    choice: ReviewChoice::Edit(base),
                }),
            },
        )
        .unwrap();
}
#[test]
fn new_review_updates_undo_replay_and_recover() {
    let Some(root) = isolated_profile::enter("new_review_updates_undo_replay_and_recover") else {
        return;
    };
    let mut session = Session::new(&root);
    let id = session.create_sanger_review(&fixture()).unwrap();
    edit(&mut session, id, 'N');
    let mut next = session.sanger_review(id).unwrap().fork_for_operation();
    let other = next.call_sets()[1].0.to_owned();
    next.select_call_set(&other).unwrap();
    session.update_sanger_review(id, &next).unwrap();
    assert_eq!(session.sanger_review(id).unwrap().draft_sha256, other);
    session.undo(id).unwrap();
    assert_eq!(
        session.sanger_review(id).unwrap().decision(0),
        Some(&ReviewChoice::Edit('N'))
    );
    session.redo(id).unwrap();
    edit(&mut session, id, 'A');
    session.flush().unwrap();
    let report = replay::replay(&root).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
    let recovered = replay::recover(&root, &root.join("recovered")).unwrap();
    assert_eq!(
        SangerReview::load(&recovered[0]).unwrap().decision(0),
        Some(&ReviewChoice::Edit('A'))
    );
    let output = root.join("saved.json");
    session.save_new(id, Some(&output)).unwrap();
    let report = replay::replay(&root).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
    edit(&mut session, id, 'G');
    session.save_new(id, Some(&output)).unwrap();
    session.undo(id).unwrap();
    session.save_new(id, Some(&output)).unwrap();
    let report = replay::replay(&root).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
    let reopened = SangerReview::load(&output).unwrap();
    assert_eq!(reopened.call_sets().len(), 2);
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('A')));
}
#[test]
fn invalid_review_edit_preserves_redo_and_retained_call_sets() {
    let Some(root) =
        isolated_profile::enter("invalid_review_edit_preserves_redo_and_retained_call_sets")
    else {
        return;
    };
    let mut session = Session::new(&root);
    let id = session.create_sanger_review(&fixture()).unwrap();
    edit(&mut session, id, 'N');
    session.undo(id).unwrap();
    let before = session
        .sanger_review(id)
        .unwrap()
        .document_report(false)
        .unwrap()
        .raw;
    let call_set = session.sanger_review(id).unwrap().draft_sha256.clone();
    assert!(
        session
            .apply(
                id,
                Op::SangerReview {
                    call_set,
                    edit: Box::new(ReviewOp::Set {
                        columns: vec![usize::MAX],
                        choice: ReviewChoice::Accept
                    })
                }
            )
            .is_err()
    );
    assert_eq!(
        session
            .sanger_review(id)
            .unwrap()
            .document_report(false)
            .unwrap()
            .raw,
        before
    );
    assert!(session.can_redo(id));
    session.redo(id).unwrap();
    assert_eq!(
        session.sanger_review(id).unwrap().decision(0),
        Some(&ReviewChoice::Edit('N'))
    );
}

#[test]
fn instrument_review_moves_with_its_sources_and_shared_saved_checkpoint() {
    let Some(root) = isolated_profile::enter(
        "instrument_review_moves_with_its_sources_and_shared_saved_checkpoint",
    ) else {
        return;
    };
    let original = root.join("original");
    std::fs::create_dir(&original).unwrap();
    let trace = original.join("trace.ab1");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &trace,
    )
    .unwrap();
    let review = basecraft_ops::sanger_review::instrument_review(&trace).unwrap();
    let mut session = Session::new(&original);
    let id = session.create_sanger_review(&review).unwrap();
    edit(&mut session, id, 'N');
    let saved = original.join("review.json");
    session.save_new(id, Some(&saved)).unwrap();
    assert!(!session.document_record(id).unwrap().unsaved);
    assert_eq!(session.saved_source(id).unwrap().unwrap().0, saved);
    drop(session);
    let moved = root.join("moved");
    std::fs::rename(&original, &moved).unwrap();
    let reopened = SangerReview::load(&moved.join("review.json")).unwrap();
    let draft = reopened.working_draft().unwrap();
    assert_eq!(
        draft.inputs[0].path.canonicalize().unwrap(),
        moved.join("trace.ab1")
    );
    assert_eq!(reopened.decision(0), Some(&ReviewChoice::Edit('N')));
}

#[test]
fn opening_legacy_review_restores_selected_undo_and_replays_it() {
    let Some(root) =
        isolated_profile::enter("opening_legacy_review_restores_selected_undo_and_replays_it")
    else {
        return;
    };
    let path = root.join("review.json");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/sanger-legacy/single.json"),
        &path,
    )
    .unwrap();
    let mut legacy = SangerReview::load(&path).unwrap();
    legacy.apply(ReviewOp::Undo).unwrap();
    let mut session = Session::new(&root);
    let id = session.open(&path).unwrap();
    assert!(session.can_undo(id));
    session.undo(id).unwrap();
    assert_eq!(
        session
            .sanger_review(id)
            .unwrap()
            .document_report(false)
            .unwrap()
            .raw,
        legacy.document_report(false).unwrap().raw
    );
    session.redo(id).unwrap();
    session.save(id, None).unwrap();
    assert!(replay::replay(&root).unwrap().ok());
}
