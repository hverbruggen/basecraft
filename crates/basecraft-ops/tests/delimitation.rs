use basecraft_ops::{Session, replay};
use basecraft_protocol::{
    Op,
    delimitation::{DelimitationEdit as Edit, HypothesisOrigin, JobRecord, JobStatus},
};

const SPART: &str = include_str!("../../../testdata/fixtures/delimitation/manual.spart");

#[test]
fn edits_undo_save_reopen_and_replay_preserve_lineage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.spart");
    std::fs::write(&path, SPART).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    assert_eq!(session.delimitation(doc).hypotheses[2].unassigned.len(), 2);
    let initial = session.delimitation(doc).clone();
    let apply = |session: &mut Session, edit| {
        session
            .apply(
                doc,
                Op::DelimitationEdit {
                    edit: Box::new(edit),
                },
            )
            .unwrap()
    };
    apply(
        &mut session,
        Edit::Derive {
            source: "partition-1".into(),
            id: "edited".into(),
            name: "Edited".into(),
        },
    );
    let derived = session.delimitation(doc).hypotheses.last().unwrap();
    assert_eq!(derived.scores, []);
    assert!(derived.groups.iter().all(|g| g.scores.is_empty()));
    apply(
        &mut session,
        Edit::Rename {
            hypothesis: "edited".into(),
            name: "New name".into(),
        },
    );
    apply(
        &mut session,
        Edit::RenameGroup {
            hypothesis: "edited".into(),
            group: "group-1".into(),
            label: "A & B".into(),
        },
    );
    let mut group = derived_group(&session, doc);
    group.id = "new".into();
    group.members = ["a".into(), "d".into()].into();
    apply(
        &mut session,
        Edit::Split {
            hypothesis: "edited".into(),
            group,
        },
    );
    apply(
        &mut session,
        Edit::Reassign {
            hypothesis: "edited".into(),
            specimens: ["b".into()].into(),
            group: None,
        },
    );
    apply(
        &mut session,
        Edit::Reassign {
            hypothesis: "edited".into(),
            specimens: ["b".into()].into(),
            group: Some("new".into()),
        },
    );
    apply(
        &mut session,
        Edit::Merge {
            hypothesis: "edited".into(),
            groups: ["group-1".into(), "new".into()].into(),
            id: "merged".into(),
            label: "Together".into(),
        },
    );
    let mut cut = session.delimitation(doc).hypotheses[3].clone();
    cut.id = "cut".into();
    cut.origin = HypothesisOrigin::ManualCut;
    apply(
        &mut session,
        Edit::SaveCut {
            hypothesis: cut,
            threshold: 0.25,
            boundary: "node_age_le_threshold".into(),
        },
    );
    let mut new = session.delimitation(doc).hypotheses[3].clone();
    new.id = "manual".into();
    new.origin = HypothesisOrigin::Manual;
    apply(&mut session, Edit::Add { hypothesis: new });
    let mut view = session.delimitation(doc).view.clone();
    view.visible = vec!["edited".into(), "partition-2".into()];
    view.selected = ["a".into(), "d".into()].into();
    apply(&mut session, Edit::SetView { view });
    let expected = session.delimitation(doc).clone();
    for _ in 0..10 {
        session.undo(doc).unwrap();
    }
    assert_eq!(session.delimitation(doc), &initial);
    for _ in 0..10 {
        session.redo(doc).unwrap();
    }
    assert_eq!(session.delimitation(doc), &expected);
    for status in [
        JobStatus::Failed,
        JobStatus::Cancelled,
        JobStatus::Completed,
    ] {
        session
            .log_job_finished(
                doc,
                JobRecord {
                    id: "job".into(),
                    generation: 1,
                    status,
                    method: "fixture".into(),
                    inputs: std::collections::BTreeMap::default(),
                    outputs: std::collections::BTreeMap::default(),
                    diagnostics: vec!["retained".into()],
                    elapsed_seconds: 0.1,
                },
            )
            .unwrap();
    }
    session.save(doc, None).unwrap();
    let mut reopened = Session::unlogged(dir.path());
    let copy = reopened.open(&path).unwrap();
    assert_eq!(reopened.delimitation(copy), &expected);
    drop(session);
    let report = replay::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

fn derived_group(
    session: &Session,
    doc: basecraft_ops::DocId,
) -> basecraft_protocol::delimitation::Group {
    session.delimitation(doc).hypotheses[3].groups[0].clone()
}

#[test]
fn invalid_edits_are_transactional_and_results_are_immutable() {
    let mut work = basecraft_io::spart::parse_text(SPART).unwrap();
    let before = work.clone();
    assert!(
        basecraft_docs::delimitation::apply(
            &mut work,
            &Edit::Reassign {
                hypothesis: "partition-1".into(),
                specimens: ["a".into()].into(),
                group: None
            }
        )
        .is_err()
    );
    assert_eq!(work, before);
    basecraft_docs::delimitation::apply(
        &mut work,
        &Edit::Derive {
            source: "partition-1".into(),
            id: "edit".into(),
            name: "Edit".into(),
        },
    )
    .unwrap();
    let before = work.clone();
    assert!(
        basecraft_docs::delimitation::apply(
            &mut work,
            &Edit::Reassign {
                hypothesis: "edit".into(),
                specimens: ["missing".into()].into(),
                group: None
            }
        )
        .is_err()
    );
    assert_eq!(work, before);
}

#[test]
fn changed_exchange_file_is_not_silently_replaced_by_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.spart");
    std::fs::write(&path, SPART).unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&path).unwrap();
    session.save(doc, None).unwrap();
    std::fs::write(&path, SPART).unwrap();
    assert!(session.open(&path).is_err());
}

#[test]
fn save_as_xml_replays_the_selected_exchange_format() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("input.spart");
    std::fs::write(&text, SPART).unwrap();
    let xml = dir.path().join("export.spartxml");
    let mut session = Session::new(dir.path());
    let doc = session.open(&text).unwrap();
    session.save(doc, Some(&xml)).unwrap();
    let mut reopened = Session::unlogged(dir.path());
    let copy = reopened.open(&xml).unwrap();
    assert_eq!(reopened.delimitation(copy), session.delimitation(doc));
    drop(session);
    let report = replay::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}
