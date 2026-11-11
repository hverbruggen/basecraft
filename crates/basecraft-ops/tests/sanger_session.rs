//! Review decisions, structural continuations and recovery through the public Session.
#[path = "support/sanger_fixture.rs"]
mod sanger_fixture;

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
            .join("../../testdata/fixtures/sanger-review/multi.json"),
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
fn stale_and_failed_saves_keep_decisions_recoverable() {
    let Some(root) = isolated_profile::enter("stale_and_failed_saves_keep_decisions_recoverable")
    else {
        return;
    };
    let path = sanger_fixture::copy_to(&root.join("source"), "multi.json");
    let mut first = Session::recoverable(path.parent().unwrap()).unwrap();
    let a = first.open(&path).unwrap();
    let mut second = Session::recoverable(path.parent().unwrap()).unwrap();
    let b = second.open(&path).unwrap();
    edit(&mut first, a, 'N');
    edit(&mut second, b, 'A');
    first.save(a, None).unwrap();
    let accepted = std::fs::read(&path).unwrap();
    assert!(second.save(b, None).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), accepted);
    let blocked = root.join("blocked.json");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("unrelated"), b"keep").unwrap();
    assert!(second.save_new(b, Some(&blocked)).is_err());
    assert_eq!(std::fs::read(blocked.join("unrelated")).unwrap(), b"keep");
    let recovered = replay::recover(second.recovery_root(), &root.join("recovered")).unwrap();
    let review = SangerReview::load(&recovered[0]).unwrap();
    assert_eq!(review.call_sets().len(), 2);
    assert_eq!(review.decision(0), Some(&ReviewChoice::Edit('A')));
    assert_eq!(std::fs::read(path).unwrap(), accepted);
}

#[test]
fn structural_redo_keeps_decisions_and_shared_ancestry() {
    let Some(root) = isolated_profile::enter("structural_redo_keeps_decisions_and_shared_ancestry")
    else {
        return;
    };
    let path = sanger_fixture::copy_to(&root, "multi.json");
    let mut session = Session::recoverable(&root).unwrap();
    let id = session.open(&path).unwrap();
    let mut selected = session.sanger_review(id).unwrap().clone();
    selected
        .select_call_set(&selected.call_sets()[1].0.to_owned())
        .unwrap();
    session.update_sanger_review(id, &selected).unwrap();
    edit(&mut session, id, 'N');
    session.undo(id).unwrap();
    session.undo(id).unwrap();
    session.redo(id).unwrap();
    session.redo(id).unwrap();
    assert_eq!(
        session.sanger_review(id).unwrap().decision(0),
        Some(&ReviewChoice::Edit('N'))
    );
    session.save(id, None).unwrap();
    drop(session);
    assert_eq!(
        SangerReview::load(&path).unwrap().decision(0),
        Some(&ReviewChoice::Edit('N'))
    );
    let first = basecraft_ops::provenance::inspect(&path).unwrap();
    let first_decision = first
        .events
        .iter()
        .find(|event| matches!(event.op, Op::SangerReview { .. }))
        .unwrap()
        .id
        .clone();
    let mut session = Session::recoverable(&root).unwrap();
    let id = session.open(&path).unwrap();
    edit(&mut session, id, 'A');
    session.save(id, None).unwrap();
    let record = basecraft_ops::document_record::DocumentRecord::from_file(&path).unwrap();
    assert!(
        record
            .report()
            .unwrap()
            .history
            .iter()
            .any(|row| row.id == first_decision)
    );
    assert_eq!(
        SangerReview::load(&path).unwrap().decision(0),
        Some(&ReviewChoice::Edit('A'))
    );
}

#[test]
fn support_survives_private_history_and_recovery_publication() {
    let Some(root) =
        isolated_profile::enter("support_survives_private_history_and_recovery_publication")
    else {
        return;
    };
    use basecraft_platform::settings::{Change, ProvenancePolicy, SettingsStore};
    let settings = SettingsStore::new(
        basecraft_platform::paths::AppPaths::discover()
            .unwrap()
            .preferences(),
    );
    for (name, policy) in [
        ("local", ProvenancePolicy::LocalOnly),
        ("none", ProvenancePolicy::None),
    ] {
        settings.update(Change::Provenance(policy)).unwrap();
        let folder = root.join(name);
        std::fs::create_dir(&folder).unwrap();
        let mut session = Session::recoverable(&folder).unwrap();
        let id = session.create_sanger_review(&fixture()).unwrap();
        let target = folder.join("review.json");
        edit(&mut session, id, 'N');
        session.save_new(id, Some(&target)).unwrap();
        edit(&mut session, id, 'A');
        session.save_new(id, Some(&target)).unwrap();
        assert_eq!(
            SangerReview::load(&target).unwrap().decision(0),
            Some(&ReviewChoice::Edit('A'))
        );
        if policy == ProvenancePolicy::LocalOnly {
            let record = basecraft_ops::document_record::DocumentRecord::from_file(&target).unwrap();
            assert!(record.events.iter().any(|event| matches!(event.op, Op::SangerReview { .. })));
        }
        edit(&mut session, id, 'T');
        let output = replay::recover(session.recovery_root(), &folder.join("recovered")).unwrap();
        let mut recovered = Session::recoverable(output[0].parent().unwrap()).unwrap();
        let id = recovered.open(&output[0]).unwrap();
        edit(&mut recovered, id, 'G');
        recovered.save(id, None).unwrap();
        assert_eq!(
            SangerReview::load(&output[0]).unwrap().decision(0),
            Some(&ReviewChoice::Edit('G'))
        );
    }
}
