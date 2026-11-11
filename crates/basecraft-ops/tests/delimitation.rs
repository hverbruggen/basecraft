#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{Session, records};
use basecraft_protocol::{
    Op,
    delimitation::{DelimitationEdit as Edit, HypothesisOrigin},
    records::{JobRecord, JobStatus},
};

const SPART: &str = include_str!("../../../testdata/fixtures/delimitation/manual.spart");

#[test]
fn edits_undo_save_reopen_and_replay_preserve_lineage() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.spart");
    std::fs::write(&path, SPART).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    assert_eq!(
        session.delimitation(doc).unwrap().hypotheses[2]
            .unassigned
            .len(),
        2
    );
    let initial = session.delimitation(doc).unwrap().clone();
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
    let derived = session
        .delimitation(doc)
        .unwrap()
        .hypotheses
        .last()
        .unwrap();
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
        Edit::Rename {
            hypothesis: "partition-1".into(),
            name: "Imported, renamed".into(),
        },
    );
    assert_eq!(
        session.delimitation(doc).unwrap().hypotheses[0].name,
        "Imported, renamed"
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
    let mut cut = session.delimitation(doc).unwrap().hypotheses[3].clone();
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
    let mut new = session.delimitation(doc).unwrap().hypotheses[3].clone();
    new.id = "manual".into();
    new.origin = HypothesisOrigin::Manual;
    apply(&mut session, Edit::Add { hypothesis: new });
    let mut view = session.delimitation(doc).unwrap().view.clone();
    view.visible = vec!["edited".into(), "partition-2".into()];
    view.selected = ["a".into(), "d".into()].into();
    apply(&mut session, Edit::SetView { view });
    let expected = session.delimitation(doc).unwrap().clone();
    for _ in 0..11 {
        session.undo(doc).unwrap();
    }
    assert_eq!(session.delimitation(doc).unwrap(), &initial);
    for _ in 0..11 {
        session.redo(doc).unwrap();
    }
    assert_eq!(session.delimitation(doc).unwrap(), &expected);
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
    assert_eq!(reopened.delimitation(copy).unwrap(), &expected);
    drop(session);
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

fn derived_group(
    session: &Session,
    doc: basecraft_ops::DocId,
) -> basecraft_protocol::delimitation::Group {
    session.delimitation(doc).unwrap().hypotheses[3].groups[0].clone()
}

#[test]
fn invalid_edits_are_transactional_and_results_are_immutable() {
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("input.spart");
    std::fs::write(&text, SPART).unwrap();
    let xml = dir.path().join("export.spartxml");
    let mut session = Session::new(dir.path());
    let doc = session.open(&text).unwrap();
    session.save(doc, Some(&xml)).unwrap();
    let mut reopened = Session::unlogged(dir.path());
    let copy = reopened.open(&xml).unwrap();
    assert_eq!(
        reopened.delimitation(copy).unwrap(),
        session.delimitation(doc).unwrap()
    );
    drop(session);
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn combine_keeps_only_selected_hypotheses_and_remove_guards_lineage() {
    use std::collections::{BTreeMap, BTreeSet};
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.spart");
    let second = dir.path().join("second.spart");
    std::fs::write(&first, SPART).unwrap();
    std::fs::write(&second, SPART).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&first).unwrap();
    let ids: Vec<String> = session
        .delimitation(doc)
        .unwrap()
        .hypotheses
        .iter()
        .map(|h| h.id.clone())
        .collect();
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
    let keep: BTreeMap<_, BTreeSet<_>> = [(second.clone(), [ids[1].clone()].into())].into();
    basecraft_ops::delimitation::combine_selected(
        &mut session,
        doc,
        std::slice::from_ref(&second),
        Some(&keep),
    )
    .unwrap();
    let combined = session.delimitation(doc).unwrap().clone();
    assert_eq!(combined.hypotheses.len(), ids.len() + 1);
    let added = combined.hypotheses.last().unwrap();
    assert_eq!(
        added.provenance.parameters["combined_source"]["hypothesis"],
        ids[1].as_str()
    );
    apply(
        &mut session,
        Edit::Derive {
            source: ids[0].clone(),
            id: "copy".into(),
            name: "Copy".into(),
        },
    );
    let refused = session.apply(
        doc,
        Op::DelimitationEdit {
            edit: Box::new(Edit::Remove {
                hypotheses: [ids[0].clone()].into(),
            }),
        },
    );
    assert!(refused.is_err());
    apply(
        &mut session,
        Edit::Remove {
            hypotheses: ["copy".to_string(), ids[0].clone()].into(),
        },
    );
    let work = session.delimitation(doc).unwrap();
    assert!(
        !work
            .hypotheses
            .iter()
            .any(|h| h.id == ids[0] || h.id == "copy")
    );
    assert!(!work.view.visible.contains(&ids[0]));
    drop(session);
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn combine_is_atomic_undoable_and_replayable() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.spart");
    let second = dir.path().join("second.spart");
    std::fs::write(&first, SPART).unwrap();
    std::fs::write(&second, SPART).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&first).unwrap();
    let original = session.delimitation(doc).unwrap().clone();
    basecraft_ops::delimitation::combine(&mut session, doc, std::slice::from_ref(&second)).unwrap();
    let combined = session.delimitation(doc).unwrap().clone();
    assert_eq!(combined.hypotheses.len(), original.hypotheses.len() * 2);
    assert_eq!(
        combined.view.visible,
        combined
            .hypotheses
            .iter()
            .map(|h| h.id.clone())
            .collect::<Vec<_>>()
    );
    for hypothesis in &combined.hypotheses[original.hypotheses.len()..] {
        assert!(
            hypothesis
                .provenance
                .parameters
                .contains_key("combined_source")
        );
        assert!(!original.hypotheses.iter().any(|h| h.id == hypothesis.id));
    }
    session.undo(doc).unwrap();
    assert_eq!(session.delimitation(doc).unwrap(), &original);
    session.redo(doc).unwrap();
    assert_eq!(session.delimitation(doc).unwrap(), &combined);
    let output = dir.path().join("combined.spart");
    session.save(doc, Some(&output)).unwrap();
    assert_eq!(
        basecraft_ops::files::load_delimitation(&output).unwrap(),
        combined
    );
    assert_eq!(std::fs::read_to_string(&first).unwrap(), SPART);
    drop(session);
    assert!(records::replay(dir.path()).unwrap().ok());
}

#[test]
fn combine_rejects_a_bad_later_source_without_importing_earlier_sources() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.spart");
    let bad = dir.path().join("bad.spart");
    std::fs::write(&first, SPART).unwrap();
    std::fs::write(&bad, "not a species hypothesis file").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&first).unwrap();
    let original = session.delimitation(doc).unwrap().clone();
    assert!(basecraft_ops::delimitation::combine(&mut session, doc, &[first, bad]).is_err());
    assert_eq!(session.delimitation(doc).unwrap(), &original);
}

#[test]
fn combine_preserves_parent_mapping_and_rejects_mismatched_specimens() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.spart");
    let second = dir.path().join("second.spart");
    let mismatch = dir.path().join("mismatch.spart");
    for path in [&first, &second] {
        std::fs::write(path, SPART).unwrap();
    }
    std::fs::write(&mismatch, SPART.replace("\na :", "\nother :")).unwrap();
    let mut source = Session::new(dir.path());
    let input = source.open(&second).unwrap();
    source
        .apply(
            input,
            Op::DelimitationEdit {
                edit: Box::new(Edit::Derive {
                    source: "partition-1".into(),
                    id: "derived".into(),
                    name: "Manual copy".into(),
                }),
            },
        )
        .unwrap();
    source.save(input, None).unwrap();
    let doc = source.open(&first).unwrap();
    let original = source.delimitation(doc).unwrap().clone();
    assert!(
        basecraft_ops::delimitation::combine(&mut source, doc, &[second.clone(), mismatch])
            .is_err()
    );
    assert_eq!(source.delimitation(doc).unwrap(), &original);
    basecraft_ops::delimitation::combine(&mut source, doc, &[second]).unwrap();
    let imported = &source.delimitation(doc).unwrap().hypotheses[original.hypotheses.len()..];
    assert_eq!(
        imported.last().unwrap().parent.as_ref(),
        Some(&imported[0].id)
    );
    assert_eq!(imported[0].scores, original.hypotheses[0].scores);
}

#[test]
fn hidden_workspace_moves_with_its_folder_and_passive_open_writes_nothing() {
    crate::isolated_profile::isolate!();
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("original");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("species.spart");
    let tree = root.join("tree.nwk");
    std::fs::write(&path, SPART).unwrap();
    std::fs::write(&tree, "((a:1,b:1):1,(c:1,d:1):1);\n").unwrap();
    let mut session = Session::new(&root);
    let doc = session.open(&path).unwrap();
    assert!(!root.join(".basecraft").exists());
    let hash = basecraft_io::hash_file(&tree).unwrap();
    let run = basecraft_ops::delimitation::reference_input(&path, &tree, &hash, None).unwrap();
    // The generic writer extraction must retain species reference-only semantics
    // and its established publication origin; no automatic source snapshot.
    let input = basecraft_ops::records::read_input(run.directory()).unwrap();
    assert_eq!(
        input.state,
        basecraft_protocol::input_records::EvidenceState::Referenced
    );
    assert_eq!(input.source.as_ref().unwrap().hash, hash);
    assert!(hash.starts_with("b3:"));
    assert!(!run.directory().join("preserved-input").exists());
    let record: basecraft_protocol::provenance::Publication =
        serde_json::from_slice(&std::fs::read(run.directory().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(record.origin, "Explicit species workspace input reference");
    assert!(record.inputs[0].retained.is_none());

    session
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(Edit::SetDisplayTree {
                    path: tree.display().to_string(),
                    hash,
                    evidence_run: run.directory().display().to_string(),
                }),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert!(!root.join("species.spart.basecraft.json").exists());
    assert!(!root.join("species.spart.provenance").exists());
    assert!(
        basecraft_ops::files::tree_sidecar(&path).starts_with(root.join(".basecraft/analyses-v1"))
    );
    session.undo(doc).unwrap();
    assert_eq!(session.delimitation(doc).unwrap().view.display_tree, None);
    session.redo(doc).unwrap();
    drop(session);
    let replay = records::replay(&root).unwrap();
    assert!(replay.ok(), "{:?}", replay.problems);
    let moved = parent.path().join("moved");
    std::fs::rename(&root, &moved).unwrap();
    let work = basecraft_ops::files::load_delimitation(&moved.join("species.spart")).unwrap();
    assert_eq!(
        basecraft_ops::delimitation::checked_display_tree(&work)
            .unwrap()
            .unwrap()
            .canonicalize()
            .unwrap(),
        moved.join("tree.nwk")
    );
    let before = std::fs::read(basecraft_ops::files::tree_sidecar(
        &moved.join("species.spart"),
    ))
    .unwrap();
    let mut reopened = Session::unlogged(&moved);
    reopened.open(&moved.join("species.spart")).unwrap();
    assert_eq!(
        before,
        std::fs::read(basecraft_ops::files::tree_sidecar(
            &moved.join("species.spart")
        ))
        .unwrap()
    );
}

#[test]
fn damaged_species_binding_refuses_legacy_fallback_and_foreign_store_refuses_save() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("species.spart");
    std::fs::write(&path, SPART).unwrap();
    let mut session = Session::unlogged(root.path());
    let doc = session.open(&path).unwrap();
    std::fs::create_dir_all(root.path().join(".basecraft/analyses-v1")).unwrap();
    assert!(session.save(doc, None).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SPART);
    assert!(!root.path().join("species.spart.basecraft.json").exists());
    let safe = tempfile::tempdir().unwrap();
    let path = safe.path().join("species.spart");
    std::fs::write(&path, SPART).unwrap();
    let mut session = Session::unlogged(safe.path());
    let doc = session.open(&path).unwrap();
    session.save(doc, None).unwrap();
    let bindings = safe.path().join(".basecraft/analyses-v1/bindings");
    for entry in std::fs::read_dir(bindings).unwrap() {
        std::fs::write(entry.unwrap().path(), "broken").unwrap();
    }
    assert!(basecraft_ops::files::load_delimitation(&path).is_err());
}

#[test]
fn referenced_display_preserve_copy_is_idempotent_and_never_uses_changed_bytes() {
    use basecraft_ops::records::{SavePolicy, conflicts, resolve};
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("species.spart");
    let tree = root.path().join("tree.nwk");
    std::fs::write(&path, SPART).unwrap();
    let original = "((a:1,b:1):1,(c:1,d:1):1);\n";
    std::fs::write(&tree, original).unwrap();
    let hash = basecraft_io::hash_file(&tree).unwrap();
    let run = basecraft_ops::delimitation::reference_input(&path, &tree, &hash, None).unwrap();
    let mut work = basecraft_io::spart::parse_text(SPART).unwrap();
    basecraft_docs::delimitation::apply(
        &mut work,
        &Edit::SetDisplayTree {
            path: tree.display().to_string(),
            hash,
            evidence_run: run.directory().display().to_string(),
        },
    )
    .unwrap();
    let conflict = conflicts(&tree).unwrap();
    assert!(resolve(&conflict, SavePolicy::Cancel).is_err());
    resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    std::fs::write(&tree, "(changed:1,bytes:1);\n").unwrap();
    resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    assert_eq!(
        std::fs::read_to_string(
            basecraft_ops::delimitation::checked_display_tree(&work)
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        original
    );
    std::fs::write(run.directory().join("preserved-input"), "damaged").unwrap();
    assert!(resolve(&conflict, SavePolicy::PreserveCopy).is_err());
    assert!(basecraft_ops::delimitation::checked_display_tree(&work).is_err());
}

#[test]
fn application_state_replay_handles_hidden_baseline_and_missing_original_folder() {
    crate::isolated_profile::isolate!();
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("species.spart");
    std::fs::write(&path, SPART).unwrap();
    let mut initial = Session::recoverable(&root).unwrap();
    let doc = initial.open(&path).unwrap();
    initial
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(Edit::Derive {
                    source: "partition-1".into(),
                    id: "editable".into(),
                    name: "Saved original".into(),
                }),
            },
        )
        .unwrap();
    initial.save(doc, None).unwrap();
    drop(initial);
    let mut edited = Session::recoverable(&root).unwrap();
    let doc = edited.open(&path).unwrap();
    edited
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(Edit::Rename {
                    hypothesis: "editable".into(),
                    name: "Saved new".into(),
                }),
            },
        )
        .unwrap();
    edited.save(doc, None).unwrap();
    // Leave an unsaved edit so application-state evidence remains owned/live.
    edited
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(Edit::Rename {
                    hypothesis: "editable".into(),
                    name: "Pending".into(),
                }),
            },
        )
        .unwrap();
    let recovery = edited.recovery_root().to_path_buf();
    std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
    let report = records::replay(&recovery).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
    let recovered = parent.path().join("recovered");
    // A missing folder alone does not concern a new destination. An unreadable
    // pending index update leaves relevance unknown and needs consent.
    let dirty = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-inputs/dirty");
    std::fs::create_dir_all(&dirty).unwrap();
    std::fs::write(dirty.join("unreadable"), b"damaged").unwrap();
    assert!(matches!(
        records::recover(&recovery, &recovered),
        Err(basecraft_ops::Error::SaveCheckRequired(_))
    ));
    assert!(!recovered.exists());
    let journal_before = basecraft_ops::log::read_all(&recovery).unwrap();
    let mut attempt =
        records::prepare_recovery(&recovery, &recovered, records::RecoveryMode::Strict).unwrap();
    let check = attempt.check(0).unwrap();
    assert_ne!(
        check.evidence().issues,
        [] as [basecraft_ops::records::InspectionIssue; 0]
    );
    attempt
        .advance(
            0,
            Some(check.decide(basecraft_ops::records::SavePolicy::Cancel, true)),
        )
        .unwrap();
    let completed = attempt.publish().unwrap();
    assert!(!completed.warnings.is_empty());
    assert_eq!(
        serde_json::to_vec(&basecraft_ops::log::read_all(&recovery).unwrap()).unwrap(),
        serde_json::to_vec(&journal_before).unwrap()
    );
    let work = basecraft_ops::files::load_delimitation(&recovered.join("species.spart")).unwrap();
    assert_eq!(work.hypotheses.last().unwrap().name, "Pending");
}

#[test]
fn visible_workspace_metadata_remains_read_only() {
    use std::io::Write;
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("legacy.spart");
    std::fs::write(&path, SPART).unwrap();
    let work = basecraft_io::spart::parse_text(SPART).unwrap();
    let hash = basecraft_io::hash_file(&path).unwrap();
    let sidecar = basecraft_ops::files::tree_sidecar(&path);
    let mut bytes = Vec::new();
    basecraft_ops::files::write_delimitation_sidecar_at(&work, &hash, &path, &mut bytes).unwrap();
    std::fs::File::create(&sidecar)
        .unwrap()
        .write_all(&bytes)
        .unwrap();
    assert_eq!(
        basecraft_ops::files::load_delimitation(&path).unwrap(),
        work
    );
    assert_eq!(std::fs::read(&sidecar).unwrap(), bytes);
    assert!(!root.path().join(".basecraft").exists());
}

#[test]
fn combined_workspace_move_and_complete_export_keep_declared_source_evidence() {
    crate::isolated_profile::isolate!();
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let first = root.join("first.spart");
    let second = root.join("second.spart");
    std::fs::write(&first, SPART).unwrap();
    std::fs::write(&second, SPART).unwrap();
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&first).unwrap();
    basecraft_ops::delimitation::combine(&mut session, doc, std::slice::from_ref(&second)).unwrap();
    let canonical = root.join("combined.spart");
    session.save(doc, Some(&canonical)).unwrap();
    let expected = session
        .delimitation(doc)
        .unwrap()
        .hypotheses
        .iter()
        .map(|h| h.groups.clone())
        .collect::<Vec<_>>();
    assert!(
        !basecraft_ops::records::conflicts(&second)
            .unwrap()
            .analyses
            .is_empty()
    );
    let package = parent.path().join("package");
    basecraft_ops::records::package(&canonical, &package, None).unwrap();
    drop(session);
    std::fs::rename(&root, parent.path().join("moved")).unwrap();
    let exported = package.join("combined.spart");
    assert_eq!(
        basecraft_ops::records::verify(&exported, None).unwrap(),
        Vec::<String>::new()
    );
    let work = basecraft_ops::files::load_delimitation(&exported).unwrap();
    assert_eq!(
        work.hypotheses
            .iter()
            .map(|h| h.groups.clone())
            .collect::<Vec<_>>(),
        expected
    );
    for hypothesis in &work.hypotheses[3..] {
        let source = hypothesis.provenance.parameters["combined_source"]["path"]
            .as_str()
            .unwrap();
        assert!(std::path::Path::new(source).is_file(), "{source}");
    }
}
