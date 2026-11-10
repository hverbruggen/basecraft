//! The GUI and headless adapters use these same ops entry points.
use basecraft_ops::{Session, delimitation as workflow, files};
use basecraft_protocol::{Actor, Op, delimitation::HypothesisOrigin};
use std::{collections::BTreeSet, path::PathBuf};

const SPART: &str = include_str!("../../../testdata/fixtures/delimitation/manual.spart");

fn inputs(dir: &std::path::Path) -> Vec<PathBuf> {
    ["specimens.asap01.spart", "specimens.gmyc01.spart"]
        .into_iter()
        .map(|name| {
            let path = dir.join(name);
            std::fs::write(&path, SPART).unwrap();
            path
        })
        .collect()
}

#[test]
fn inspection_and_naming_are_read_only_and_keep_stable_order() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    let mut work = files::load_delimitation(&paths[0]).unwrap();
    work.hypotheses[0].origin = HypothesisOrigin::Manual;
    work.hypotheses[0].scores.clear();
    for group in &mut work.hypotheses[0].groups {
        group.scores.clear();
    }
    work.hypotheses[1].origin = HypothesisOrigin::Inferred;
    work.hypotheses[1]
        .attributes
        .insert("asap_rank".into(), "2".into());
    work.hypotheses[2].origin = HypothesisOrigin::Inferred;
    work.hypotheses[2]
        .attributes
        .insert("asap_rank".into(), "1".into());
    let hash = basecraft_io::hash_file(&paths[0]).unwrap();
    files::write_delimitation_sidecar_at(
        &work,
        &hash,
        &paths[0],
        &mut std::fs::File::create(files::tree_sidecar(&paths[0])).unwrap(),
    )
    .unwrap();
    let before = std::fs::read_dir(dir.path()).unwrap().count();
    let files = workflow::combine_candidates(&paths).unwrap();
    let ids: Vec<_> = files[0]
        .hypotheses
        .iter()
        .map(|h| (&h.hypothesis.id, h.best))
        .collect();
    assert_eq!(
        ids,
        vec![
            (&work.hypotheses[2].id, true),
            (&work.hypotheses[1].id, false),
            (&work.hypotheses[0].id, false)
        ]
    );
    assert_eq!(files[0].specimens, 4);
    assert_eq!(
        workflow::combined_path(&paths),
        dir.path().join("specimens.combined01.spart")
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), before);
    std::fs::write(dir.path().join("specimens.combined01.spart"), "reserved").unwrap();
    assert_eq!(
        workflow::combined_path(&paths),
        dir.path().join("specimens.combined02.spart")
    );
    assert!(workflow::combined_path(&[]).as_os_str().is_empty());
}

#[test]
fn selected_combination_preserves_sources_actor_lineage_and_replay() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    let output = dir.path().join("combined.spart");
    let keep = [
        (paths[0].clone(), BTreeSet::from(["partition-2".into()])),
        (paths[1].clone(), BTreeSet::from(["partition-3".into()])),
    ]
    .into();
    let mut session = Session::new(dir.path());
    session.set_actor(Actor::Script {
        client: "workflow-test".into(),
    });
    let doc = workflow::combine_to(&mut session, &paths, &output, Some(&keep)).unwrap();
    let work = session.delimitation(doc);
    assert_eq!(work.hypotheses.len(), 2);
    assert_eq!(work.hypotheses[0].id, "partition-2");
    assert_eq!(work.hypotheses[1].unassigned.len(), 2);
    let source = &work.hypotheses[1].provenance.parameters["combined_source"];
    assert_eq!(source["hypothesis"], "partition-3");
    assert_eq!(source["hash"], basecraft_io::hash_file(&paths[1]).unwrap());
    let reopened = files::load_delimitation(&output).unwrap();
    assert_eq!(&reopened, work);
    for path in &paths {
        assert_eq!(std::fs::read_to_string(path).unwrap(), SPART);
    }
    let record = basecraft_ops::provenance::inspect(&output).unwrap();
    assert!(
        record
            .events
            .iter()
            .any(|e| matches!(e.op, Op::DelimitationEdit { .. })
                && matches!(&e.actor, Actor::Script { client } if client == "workflow-test"))
    );
    session.undo(doc).unwrap();
    assert_eq!(session.delimitation(doc).hypotheses.len(), 3);
    assert_eq!(session.delimitation(doc).hypotheses[0].id, "partition-1");
    session.redo(doc).unwrap();
    assert_eq!(session.delimitation(doc), &reopened);
    let replay = basecraft_ops::replay::replay(dir.path()).unwrap();
    assert!(replay.problems.is_empty(), "{:?}", replay.problems);
}

#[test]
fn explicit_selection_can_keep_only_the_first_file() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    let output = dir.path().join("only-first.spart.xml");
    let keep = [(paths[0].clone(), BTreeSet::from(["partition-2".into()]))].into();
    workflow::combine_to(&mut Session::new(dir.path()), &paths, &output, Some(&keep)).unwrap();
    let work = files::load_delimitation(&output).unwrap();
    assert_eq!(work.hypotheses.len(), 1);
    assert_eq!(work.hypotheses[0].id, "partition-2");
    assert!(std::fs::read_to_string(output).unwrap().contains("<?xml"));
}

#[test]
fn rejects_bad_selections_wrong_kind_roster_and_existing_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    let output = dir.path().join("combined.spart");
    let mut session = Session::new(dir.path());
    assert!(workflow::combine_to(&mut session, &paths[..1], &output, None).is_err());
    assert!(
        workflow::combine_to(
            &mut session,
            &paths,
            &output,
            Some(&workflow::CombineSelection::default())
        )
        .is_err()
    );
    for path in &paths {
        let keep = [(path.clone(), BTreeSet::from(["missing".into()]))].into();
        assert!(workflow::combine_to(&mut session, &paths, &output, Some(&keep)).is_err());
    }
    let keep = [(
        dir.path().join("other.spart"),
        BTreeSet::from(["partition-1".into()]),
    )]
    .into();
    assert!(workflow::combine_to(&mut session, &paths, &output, Some(&keep)).is_err());
    assert!(!output.exists());
    std::fs::write(&paths[1], SPART.replace("d :", "different :")).unwrap();
    assert!(workflow::combine_to(&mut session, &paths, &output, None).is_err());
    assert!(!output.exists());
    let fasta = dir.path().join("sequence.fa");
    std::fs::write(&fasta, ">a\nACGT\n").unwrap();
    assert!(workflow::combine_to(&mut session, &[fasta, paths[0].clone()], &output, None).is_err());
    for target in [&output, &files::tree_sidecar(&output)] {
        std::fs::write(target, "keep").unwrap();
        assert!(workflow::combine_to(&mut session, &paths, &output, None).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
        std::fs::remove_file(target).unwrap();
    }
}

#[test]
fn omitted_selection_keeps_every_hypothesis_and_failed_save_keeps_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    let mut session = Session::new(dir.path());
    let output = dir.path().join("all.spart");
    let doc = workflow::combine_to(&mut session, &paths, &output, None).unwrap();
    assert_eq!(session.delimitation(doc).hypotheses.len(), 6);
    drop(session);
    let failed = tempfile::tempdir().unwrap();
    let paths = inputs(failed.path());
    let mut session = Session::new(failed.path());
    // The shared writer refuses to save hypotheses as a FASTA after the edits
    // were journalled. The unsuccessful destination leaves replayable recovery.
    let bad_output = failed.path().join("failed.fa");
    assert!(workflow::combine_to(&mut session, &paths, &bad_output, None).is_err());
    let replay = basecraft_ops::replay::replay(failed.path()).unwrap();
    assert!(replay.entries >= 2);
    assert!(replay.problems.is_empty(), "{:?}", replay.problems);
    for path in paths {
        assert_eq!(std::fs::read_to_string(path).unwrap(), SPART);
    }
    assert!(!bad_output.exists());
}
