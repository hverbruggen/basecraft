//! Review decisions, structural continuations and recovery through the public Session.
#[path = "support/sanger_fixture.rs"]
mod sanger_fixture;

use basecraft_ops::{Session, records, sanger_review::SangerReview};
use basecraft_protocol::{
    Op,
    sanger::{ReviewChoice, ReviewOp},
};
use std::path::Path;
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

/// Loads a copy of the fixture review, so the records basecraft writes beside
/// its traces land in the test's folder and never in the source tree.
fn fixture(root: &Path) -> SangerReview {
    let path = sanger_fixture::copy_to(&root.join("fixture"), "multi.json");
    SangerReview::load(&path).unwrap()
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
    let id = session.create_sanger_review(&fixture(&root)).unwrap();
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
    let report = records::replay(&root).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
    let recovered = records::recover(&root, &root.join("recovered"), None).unwrap();
    assert_eq!(
        SangerReview::load(&recovered[0]).unwrap().decision(0),
        Some(&ReviewChoice::Edit('A'))
    );
    let output = root.join("saved.json");
    session.save_new(id, Some(&output)).unwrap();
    let report = records::replay(&root).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
    edit(&mut session, id, 'G');
    session.save_new(id, Some(&output)).unwrap();
    session.undo(id).unwrap();
    session.save_new(id, Some(&output)).unwrap();
    let report = records::replay(&root).unwrap();
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
    let id = session.create_sanger_review(&fixture(&root)).unwrap();
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
    assert!(session.can_redo(id).unwrap());
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
    let recovered =
        records::recover(second.recovery_root(), &root.join("recovered"), None).unwrap();
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
    let call_set = selected.call_sets()[1].0.to_string();
    selected.select_call_set(&call_set).unwrap();
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
    let first = basecraft_ops::records::inspect(&path, None).unwrap();
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
    let record = basecraft_ops::records::DocumentRecord::from_file(&path, None).unwrap();
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
fn support_survives_recovery_publication() {
    let Some(root) = isolated_profile::enter("support_survives_recovery_publication") else {
        return;
    };
    let folder = root.join("review");
    std::fs::create_dir(&folder).unwrap();
    let mut session = Session::recoverable(&folder).unwrap();
    let id = session.create_sanger_review(&fixture(&root)).unwrap();
    let target = folder.join("review.json");
    edit(&mut session, id, 'N');
    session.save_new(id, Some(&target)).unwrap();
    edit(&mut session, id, 'A');
    session.save_new(id, Some(&target)).unwrap();
    assert_eq!(
        SangerReview::load(&target).unwrap().decision(0),
        Some(&ReviewChoice::Edit('A'))
    );
    edit(&mut session, id, 'T');
    let output =
        records::recover(session.recovery_root(), &folder.join("recovered"), None).unwrap();
    let mut recovered = Session::recoverable(output[0].parent().unwrap()).unwrap();
    let id = recovered.open(&output[0]).unwrap();
    edit(&mut recovered, id, 'G');
    recovered.save(id, None).unwrap();
    assert_eq!(
        SangerReview::load(&output[0]).unwrap().decision(0),
        Some(&ReviewChoice::Edit('G'))
    );
}

#[test]
fn selections_are_small_and_undo_restores_every_saved_byte() {
    let Some(root) =
        isolated_profile::enter("selections_are_small_and_undo_restores_every_saved_byte")
    else {
        return;
    };
    let mut session = Session::new(&root);
    let id = session.create_sanger_review(&fixture(&root)).unwrap();
    let selected = session.sanger_review(id).unwrap().draft_sha256.clone();
    let other = session.sanger_review(id).unwrap().call_sets()[1]
        .0
        .to_owned();
    let before = root.join("before.json");
    session.save_new(id, Some(&before)).unwrap();
    let bytes = std::fs::read(&before).unwrap();
    let entries = basecraft_ops::log::read_all(&root).unwrap().len();
    for i in 0..200 {
        session
            .apply(
                id,
                Op::SangerSelect {
                    call_set: if i % 2 == 0 {
                        other.clone()
                    } else {
                        selected.clone()
                    },
                },
            )
            .unwrap();
    }
    let log = basecraft_ops::log::read_all(&root).unwrap();
    assert!(serde_json::to_vec(&log[entries..]).unwrap().len() < 200 * 1024);
    for _ in 0..200 {
        session.undo(id).unwrap();
    }
    assert_eq!(session.sanger_review(id).unwrap().draft_sha256, selected);
    let raw = session
        .sanger_review(id)
        .unwrap()
        .document_report(false)
        .unwrap()
        .raw;
    // The edit is explicitly addressed to an inactive set.
    session
        .apply(
            id,
            Op::SangerReview {
                call_set: other,
                edit: Box::new(ReviewOp::Set {
                    columns: vec![0],
                    choice: ReviewChoice::Edit('N'),
                }),
            },
        )
        .unwrap();
    assert_eq!(session.sanger_review(id).unwrap().draft_sha256, selected);
    session.undo(id).unwrap();
    assert_eq!(
        session
            .sanger_review(id)
            .unwrap()
            .document_report(false)
            .unwrap()
            .raw,
        raw
    );
    let mut settings = session.sanger_review(id).unwrap().mask.clone();
    settings.start = 1;
    session
        .apply(
            id,
            Op::SangerReview {
                call_set: selected,
                edit: Box::new(ReviewOp::Mask { settings }),
            },
        )
        .unwrap();
    session.undo(id).unwrap();
    let after = root.join("after.json");
    session.save_new(id, Some(&after)).unwrap();
    assert_eq!(std::fs::read(&after).unwrap(), bytes);
    assert!(records::replay(&root).unwrap().ok());
}

#[test]
fn content_recognition_and_unknown_handles_are_checked() {
    let Some(root) = isolated_profile::enter("content_recognition_and_unknown_handles_are_checked")
    else {
        return;
    };
    let mut session = Session::new(&root);
    let id = session.create_sanger_review(&fixture(&root)).unwrap();
    let path = root.join("review.without-json-extension");
    session.save_new(id, Some(&path)).unwrap();
    assert_eq!(
        session.open_classified(&path).unwrap().1,
        basecraft_ops::files::Kind::SangerReview
    );
    let json = root.join("unrelated.json");
    std::fs::write(&json, b"{\"ordinary\":true}").unwrap();
    assert!(matches!(
        session.open(&json),
        Err(basecraft_ops::Error::UnknownFormat(_))
    ));
    let missing = basecraft_ops::DocId(usize::MAX);
    for result in [
        session.doc_key(missing).map(|_| ()),
        session.generation(missing).map(|_| ()),
        session.revision(missing).map(|_| ()),
        session.state(missing).map(|_| ()),
        session.can_undo(missing).map(|_| ()),
        session.can_redo(missing).map(|_| ()),
        session.undo(missing).map(|_| ()),
        session.redo(missing).map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(basecraft_ops::Error::UnknownDocument { .. })
        ));
    }
}

#[test]
fn direct_analysis_update_retains_its_record_and_history_is_readable_once() {
    let Some(root) = isolated_profile::enter(
        "direct_analysis_update_retains_its_record_and_history_is_readable_once",
    ) else {
        return;
    };
    let review_path = sanger_fixture::copy_to(&root, "multi.json");
    let mut session = Session::new(&root);
    let id = session.open(&review_path).unwrap();
    let record = root.join("external-record");
    std::fs::copy(&review_path, &record).unwrap();
    let hash = basecraft_io::hash_file(&record).unwrap();
    session
        .apply(
            id,
            Op::SangerUpdate {
                review: basecraft_protocol::sanger_records::ReviewRecord {
                    path: record.clone(),
                    hash,
                },
            },
        )
        .unwrap();
    std::fs::remove_file(&record).unwrap();
    let baseline = session
        .sanger_review(id)
        .unwrap()
        .report_with_history(&session.document_record(id).unwrap())
        .unwrap();
    edit(&mut session, id, 'N');
    let report = session
        .sanger_review(id)
        .unwrap()
        .report_with_history(&session.document_record(id).unwrap())
        .unwrap();
    assert_eq!(report.history.len(), baseline.history.len() + 1);
    let row = report.history.last().unwrap();
    assert_eq!(row.summary, "Set 1 review columns to N");
    session.undo(id).unwrap();
    let report = session
        .sanger_review(id)
        .unwrap()
        .report_with_history(&session.document_record(id).unwrap())
        .unwrap();
    assert_eq!(
        report
            .history
            .iter()
            .filter(|row| row.summary == "Set 1 review columns to N")
            .count(),
        1
    );
    assert!(records::replay(&root).unwrap().ok());
    records::recover(&root, &root.join("recovered"), None).unwrap();
    session
        .save_new(id, Some(&root.join("saved.json")))
        .unwrap();
}

#[test]
fn previous_save_as_target_requires_explicit_replacement() {
    let Some(root) =
        isolated_profile::enter("previous_save_as_target_requires_explicit_replacement")
    else {
        return;
    };
    let mut session = Session::new(&root);
    let id = session.create_sanger_review(&fixture(&root)).unwrap();
    let first = root.join("first.json");
    let second = root.join("second.json");
    session.save_new(id, Some(&first)).unwrap();
    session.save_new(id, Some(&second)).unwrap();
    let bytes = std::fs::read(&first).unwrap();
    let replacement = root.join("replacement");
    std::fs::write(&replacement, &bytes).unwrap();
    std::fs::rename(replacement, &first).unwrap();
    assert!(session.save_new(id, Some(&first)).is_err());
    assert_eq!(std::fs::read(first).unwrap(), bytes);
}
