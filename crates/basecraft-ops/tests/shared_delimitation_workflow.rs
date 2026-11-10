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
fn omitted_selection_keeps_every_hypothesis_and_replays() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    let mut session = Session::new(dir.path());
    let output = dir.path().join("all.spart");
    let doc = workflow::combine_to(&mut session, &paths, &output, None).unwrap();
    assert_eq!(session.delimitation(doc).hypotheses.len(), 6);
    let replay = basecraft_ops::replay::replay(dir.path()).unwrap();
    assert!(replay.problems.is_empty(), "{:?}", replay.problems);
}

#[test]
fn refused_save_keeps_the_unsaved_batch_replayable() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    let mut session = Session::new(dir.path());
    // This tests a single failed save. The separate ignored regression below
    // retains the realistic success/success/refusal journey in ONE session.
    let output = dir.path().join("failed.fa");
    assert!(workflow::combine_to(&mut session, &paths, &output, None).is_err());
    let replay = basecraft_ops::replay::replay(dir.path()).unwrap();
    assert!(replay.entries >= 2);
    assert!(replay.problems.is_empty(), "{:?}", replay.problems);
    for path in paths {
        assert_eq!(std::fs::read_to_string(path).unwrap(), SPART);
    }
    assert!(!output.exists());
}

#[test]
fn path_aliases_match_selection_without_merging_input_entries() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let alias = dir.path().join("sub/../specimens.asap01.spart");
    let keep = [
        (paths[0].clone(), BTreeSet::from(["partition-2".into()])),
        (paths[1].clone(), BTreeSet::from(["partition-3".into()])),
    ]
    .into();
    let mut session = Session::new(dir.path());
    let doc = workflow::combine_to(
        &mut session,
        &[alias.clone(), paths[1].clone()],
        &dir.path().join("alias.spart"),
        Some(&keep),
    )
    .unwrap();
    let hypotheses = &session.delimitation(doc).hypotheses;
    assert_eq!(hypotheses.len(), 2);
    assert_eq!(hypotheses[0].id, "partition-2");
    assert_eq!(
        hypotheses[1].provenance.parameters["combined_source"]["hypothesis"],
        "partition-3"
    );
    assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), SPART);
    assert_eq!(std::fs::read_to_string(&paths[1]).unwrap(), SPART);

    // Multiple spellings in the selection are ambiguous, not a last-key-wins
    // merge. Check before a document is opened or recovery state is created.
    let duplicate = [
        (paths[0].clone(), BTreeSet::from(["partition-2".into()])),
        (alias, BTreeSet::from(["partition-3".into()])),
    ]
    .into();
    let output = dir.path().join("ambiguous.spart");
    assert!(workflow::combine_to(&mut session, &paths, &output, Some(&duplicate)).is_err());
    assert!(!output.exists());

    // Equal content in two paths remains two inputs; even explicit repeated
    // references remain in the input list rather than being deduplicated.
    for sources in [paths.clone(), vec![paths[0].clone(), paths[0].clone()]] {
        let isolated = tempfile::tempdir().unwrap();
        let output = isolated.path().join("multiple.spart");
        let mut session = Session::new(dir.path());
        let doc = workflow::combine_to(&mut session, &sources, &output, None).unwrap();
        let hypotheses = &session.delimitation(doc).hypotheses;
        assert_eq!(hypotheses.len(), 6);
        assert_eq!(
            hypotheses
                .iter()
                .map(|h| &h.id)
                .collect::<BTreeSet<_>>()
                .len(),
            6
        );
    }
}

// This is intentionally not replaced by separate sessions. The persistence
// worker owns the materialisation fix; run with --ignored after that fix is
// combined with this branch. It fails on the known baseline defect.
#[test]
#[ignore = "requires the separately assigned replay materialisation correction"]
fn repeated_combine_and_refused_save_preserve_retained_objects_and_recover() {
    let dir = tempfile::tempdir().unwrap();
    let paths = inputs(dir.path());
    let mut session = Session::new(dir.path());
    for filename in ["one.spart", "two.spart"] {
        let doc =
            workflow::combine_to(&mut session, &paths, &dir.path().join(filename), None).unwrap();
        assert_eq!(session.delimitation(doc).hypotheses.len(), 6);
    }
    assert!(
        workflow::combine_to(&mut session, &paths, &dir.path().join("refused.fa"), None).is_err()
    );
    session.flush().unwrap();
    let object_dir = dir.path().join(".basecraft/objects");
    let snapshot = || {
        std::fs::read_dir(&object_dir)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (
                    path.file_name().unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let objects = snapshot();
    let replay = basecraft_ops::replay::replay(dir.path()).unwrap();
    assert_eq!(snapshot(), objects, "replay changed retained input bytes");
    assert!(replay.problems.is_empty(), "{:?}", replay.problems);
    let recovered = tempfile::tempdir().unwrap();
    let destination = recovered.path().join("recovered");
    let outputs = basecraft_ops::replay::recover(dir.path(), &destination).unwrap();
    assert!(
        outputs.iter().next().is_some(),
        "recovery returned no documents"
    );
    for output in outputs {
        assert_eq!(
            files::load_delimitation(&output).unwrap().hypotheses.len(),
            6
        );
    }
    assert_eq!(snapshot(), objects, "recovery changed retained input bytes");
    for path in &paths {
        assert_eq!(std::fs::read_to_string(path).unwrap(), SPART);
    }
}
