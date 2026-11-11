use basecraft_ops::document_record::ReportExt;
use basecraft_ops::{Session, provenance};
use basecraft_protocol::Op;
use std::path::Path;

fn paths(root: &Path) -> basecraft_platform::paths::AppPaths {
    basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| root.join("profile").into_os_string())
    })
    .unwrap()
}
fn edit() -> Op {
    Op::SeqReplace {
        start: 1,
        end: 2,
        residues: "T".into(),
    }
}

#[test]
fn passive_open_close_and_unchanged_save_create_no_records() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let paths = paths(dir.path());
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
        let doc = session.open(&file).unwrap();
        session.save(doc, None).unwrap();
        assert_eq!(session.doc(doc).unwrap().len(), 4);
    }
    assert!(!paths.state.exists());
    assert!(!paths.cache.exists());
    assert!(!paths.config.exists());
    assert!(!dir.path().join(".basecraft").exists());
    assert!(!provenance::companion(&file).exists());
}

#[test]
fn edit_undo_redo_save_move_verify_and_report() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let paths = paths(dir.path());
    let recovery;
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
        recovery = session.recovery_root().to_path_buf();
        let doc = session.open(&file).unwrap();
        assert!(!recovery.exists());
        session.apply(doc, edit()).unwrap();
        assert!(recovery.join("recovery.json").is_file());
        assert!(!dir.path().join(".basecraft").exists());
        session.undo(doc).unwrap();
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
        assert_eq!(provenance::verify(&file).unwrap(), Vec::<String>::new());
        let publication = provenance::inspect(&file).unwrap();
        assert_eq!(publication.effective_edits.len(), 1);
        assert_eq!(publication.inputs.len(), 1);
        assert!(!publication.completeness.ancestry_known);
        assert!(!publication.completeness.replay_verified);
        assert!(
            publication
                .events
                .iter()
                .any(|e| matches!(e.op, Op::Undo { .. }))
        );
    }
    assert!(!recovery.exists(), "clean close removes published recovery");
    let exported = dir.path().join("package");
    provenance::package(&file, &exported).unwrap();
    let moved = exported.join("gene.fasta");
    std::fs::remove_file(&file).unwrap();
    std::fs::remove_dir_all(provenance::companion(&file)).unwrap();
    assert_eq!(provenance::verify(&moved).unwrap(), Vec::<String>::new());
    let reports = provenance::companion(&moved).join("reports");
    assert!(
        std::fs::read_to_string(reports.join("methods.generated.md"))
            .unwrap()
            .contains("Replaced")
    );
    std::fs::write(reports.join("methods.md"), "My reviewed prose").unwrap();
    provenance::write_reports(
        &provenance::companion(&moved),
        &provenance::inspect(&moved).unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(reports.join("methods.md")).unwrap(),
        "My reviewed prose"
    );
}

#[test]
fn completely_undone_edit_publishes_nothing_and_cleans_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let paths = paths(dir.path());
    let recovery;
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
        recovery = session.recovery_root().to_path_buf();
        let doc = session.open(&file).unwrap();
        session.apply(doc, edit()).unwrap();
        session.undo(doc).unwrap();
        session.save(doc, None).unwrap();
    }
    assert!(!provenance::companion(&file).exists());
    assert!(!recovery.exists());
}

#[test]
fn changed_source_refused_before_mutation_and_before_save() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    std::fs::write(&file, ">gene\nGGGG\n").unwrap();
    assert!(
        session
            .apply(doc, edit())
            .unwrap_err()
            .to_string()
            .contains("source changed")
    );
    assert_eq!(session.generation(doc).unwrap(), 0);
    assert!(!dir.path().join(".basecraft").exists());
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    std::fs::write(&file, ">gene\nTTTT\n").unwrap();
    assert!(matches!(
        session.save(doc, None),
        Err(basecraft_ops::Error::ChangedOnDisk { .. })
    ));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), ">gene\nTTTT\n");
}

#[test]
fn tampered_retained_original_and_path_escape_are_detected() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    session.save(doc, None).unwrap();
    let publication = provenance::inspect(&file).unwrap();
    let retained = publication.inputs[0].retained.as_ref().unwrap();
    std::fs::write(provenance::companion(&file).join(retained), b"changed").unwrap();
    assert_ne!(provenance::verify(&file).unwrap(), Vec::<String>::new());
    let mut publication = publication;
    publication.inputs[0].retained = Some("../gene.fasta".into());
    std::fs::write(
        provenance::companion(&file).join("manifest.json"),
        serde_json::to_vec(&publication).unwrap(),
    )
    .unwrap();
    assert!(
        provenance::verify(&file)
            .unwrap()
            .iter()
            .any(|e| e.contains("unsafe"))
    );
}

#[test]
fn sidecar_failure_reports_partial_save_and_keeps_pending_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    // A directory at the companion location injects a publication write error.
    let bundle = provenance::companion(&file);
    std::fs::create_dir(&bundle).unwrap();
    std::fs::write(bundle.join("artifacts"), "blocked").unwrap();
    assert!(
        session
            .save(doc, None)
            .unwrap_err()
            .to_string()
            .contains("data saved")
    );
    assert!(session.recovery_root().join("pending-save.json").exists());
    assert!(std::fs::read_to_string(&file).unwrap().contains("ATGT"));
    std::fs::remove_file(bundle.join("artifacts")).unwrap();
    session.save(doc, None).unwrap();
    assert!(!session.recovery_root().join("pending-save.json").exists());
}

#[test]
fn no_recovery_and_no_provenance_policy_leave_no_durable_lineage() {
    use basecraft_platform::settings::{Change, ProvenancePolicy, SettingsStore};
    let dir = tempfile::tempdir().unwrap();
    let paths = paths(dir.path());
    let store = SettingsStore::new(paths.preferences());
    store.update(Change::Recovery(false)).unwrap();
    store
        .update(Change::Provenance(ProvenancePolicy::None))
        .unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    session.save(doc, None).unwrap();
    assert!(!paths.state.join("recovery").exists());
    assert!(!paths.state.join("provenance").exists());
    assert!(!provenance::companion(&file).exists());
}

#[test]
fn save_as_does_not_disable_original_conflict_detection() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("a.fasta");
    let copy = dir.path().join("b.fasta");
    std::fs::write(&original, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&original).unwrap();
    session.save(doc, Some(&copy)).unwrap();
    session.apply(doc, edit()).unwrap();
    std::fs::write(&original, ">external\nAAAA\n").unwrap();
    assert!(matches!(
        session.save(doc, None),
        Err(basecraft_ops::Error::ChangedOnDisk { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(original).unwrap(),
        ">external\nAAAA\n"
    );
}

#[test]
fn live_annotation_sidecar_is_verified_and_packaged() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tree.nwk");
    std::fs::write(&file, "(A:1,B:1);\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session
        .apply(
            doc,
            Op::TreeRotate {
                clade: vec!["A".into(), "B".into()],
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    let published = provenance::inspect(&file).unwrap();
    assert_eq!(published.outputs.len(), 2);
    let package = dir.path().join("package");
    provenance::package(&file, &package).unwrap();
    assert!(package.join(&published.outputs[1].logical_name).is_file());
    let sidecar = dir.path().join(&published.outputs[1].logical_name);
    std::fs::write(sidecar, "{}").unwrap();
    assert_ne!(provenance::verify(&file).unwrap(), Vec::<String>::new());
}

#[test]
fn journal_checksum_and_torn_tail_expose_only_verified_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    session.flush().unwrap();
    let host = std::fs::read_dir(basecraft_ops::log::log_dir(dir.path()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal = std::fs::read_dir(host)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = std::fs::read(&journal).unwrap();
    let prefix = basecraft_ops::log::verified_prefix(&journal).unwrap();
    assert_eq!(prefix.entries.len(), 2);
    assert!(prefix.problem.is_none());
    let mut torn = bytes.clone();
    torn.extend_from_slice(b"{\"incomplete\"");
    std::fs::write(&journal, &torn).unwrap();
    let prefix = basecraft_ops::log::verified_prefix(&journal).unwrap();
    assert_eq!(prefix.entries.len(), 2);
    assert_eq!(prefix.verified_bytes, bytes.len() as u64);
    assert!(prefix.problem.unwrap().contains("torn"));
    // A corrupt middle event must stop recovery, never skip to later events.
    let text = String::from_utf8(bytes).unwrap().replace("ACGT", "TCGT");
    // Change a guaranteed field without updating checksum.
    let text = text.replace("seq.replace", "seq.replacf");
    std::fs::write(&journal, text).unwrap();
    let prefix = basecraft_ops::log::verified_prefix(&journal).unwrap();
    assert_eq!(prefix.entries.len(), 1);
    assert!(prefix.problem.unwrap().contains("checksum"));
}

#[test]
fn unsaved_recovery_reconstructs_exact_state_without_source_or_cache() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let recovery;
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
        recovery = session.recovery_root().to_path_buf();
        let doc = session.open(&file).unwrap();
        session.apply(doc, edit()).unwrap();
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 2,
                    end: 3,
                    residues: "A".into(),
                },
            )
            .unwrap();
        session.undo(doc).unwrap();
    }
    assert!(recovery.is_dir());
    std::fs::remove_file(&file).unwrap();
    let destination = dir.path().join("restored");
    let restored = basecraft_ops::replay::recover(&recovery, &destination).unwrap();
    assert_eq!(restored, vec![destination.join("gene.fasta")]);
    assert_eq!(
        std::fs::read_to_string(&restored[0]).unwrap(),
        ">gene\nATGT\n"
    );
    assert!(!file.exists());
    assert!(basecraft_ops::replay::recover(&recovery, &destination).is_err());
}

#[test]
fn reopen_and_save_as_preserve_contributing_ancestry() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
        let doc = session.open(&file).unwrap();
        session.apply(doc, edit()).unwrap();
        session.save(doc, None).unwrap();
    }
    let first = provenance::inspect(&file).unwrap();
    let copy = dir.path().join("copy.fasta");
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
        let doc = session.open(&file).unwrap();
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 2,
                    end: 3,
                    residues: "A".into(),
                },
            )
            .unwrap();
        session.save(doc, Some(&copy)).unwrap();
    }
    let publication = provenance::inspect(&copy).unwrap();
    assert_eq!(publication.parents, vec![first.id]);
    assert_eq!(publication.effective_edits.len(), 2);
    std::fs::remove_dir_all(provenance::companion(&file)).unwrap();
    assert_eq!(provenance::verify(&copy).unwrap(), Vec::<String>::new());
}

#[test]
fn recovery_rejects_checked_traversal_and_corrupt_retained_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    let recovery = session.recovery_root();
    let host = std::fs::read_dir(basecraft_ops::log::log_dir(recovery))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal = std::fs::read_dir(host)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original = std::fs::read_to_string(&journal).unwrap();
    for (open, key) in [
        ("../outside.fasta", "gene.fasta"),
        ("gene.fasta", "../outside.fasta"),
        ("/tmp/outside.fasta", "/tmp/outside.fasta"),
    ] {
        let mutated = original
            .lines()
            .enumerate()
            .map(|(index, line)| {
                let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
                value.as_object_mut().unwrap().remove("_checksum");
                value.as_object_mut().unwrap().remove("_sequence");
                if index == 0 {
                    value["params"]["path"] = open.into();
                    value["doc"] = key.into();
                }
                let checksum = basecraft_io::hash_bytes(&serde_json::to_vec(&value).unwrap());
                value["_checksum"] = checksum.into();
                value["_sequence"] = index.into();
                serde_json::to_string(&value).unwrap() + "\n"
            })
            .collect::<String>();
        std::fs::write(&journal, mutated).unwrap();
        assert!(
            basecraft_ops::replay::recover(recovery, &dir.path().join("output"))
                .unwrap_err()
                .to_string()
                .contains("unsafe")
        );
        assert!(!dir.path().join("outside.fasta").exists());
    }
    std::fs::write(&journal, original).unwrap();
    let baseline = basecraft_ops::log::read_all(recovery).unwrap();
    let digest = baseline[0].result.hashes[0].1.trim_start_matches("b3:");
    std::fs::write(
        basecraft_ops::session::objects_dir(recovery).join(digest),
        "corrupt",
    )
    .unwrap();
    assert!(
        basecraft_ops::replay::recover(recovery, &dir.path().join("output"))
            .unwrap_err()
            .to_string()
            .contains("hash differs")
    );
}

#[test]
fn ancestor_only_artifacts_and_duplicate_parent_references_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    session.save(doc, None).unwrap();
    let ancestor = provenance::inspect(&file).unwrap();
    session
        .apply(
            doc,
            Op::SeqReplace {
                start: 2,
                end: 3,
                residues: "A".into(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    let bundle = provenance::companion(&file);
    let old_output = bundle.join(ancestor.outputs[0].retained.as_ref().unwrap());
    let original_bytes = std::fs::read(&old_output).unwrap();
    std::fs::write(&old_output, "corrupt ancestor-only output").unwrap();
    assert!(
        provenance::verify(&file)
            .unwrap()
            .iter()
            .any(|issue| issue.contains("ancestor retained"))
    );
    std::fs::write(old_output, original_bytes).unwrap();
    let mut current = provenance::inspect(&file).unwrap();
    current.parents.push(current.parents[0].clone());
    let bytes = serde_json::to_vec(&current).unwrap();
    std::fs::write(bundle.join("manifest.json"), &bytes).unwrap();
    std::fs::write(
        bundle.join("records").join(format!("{}.json", current.id)),
        &bytes,
    )
    .unwrap();
    assert!(
        provenance::verify(&file)
            .unwrap_err()
            .to_string()
            .contains("duplicate parent")
    );
}

#[test]
fn current_record_tracks_unsaved_undo_redo_and_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let paths = paths(dir.path());
    let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let doc = session.open(&file).unwrap();
    let imported = session.document_record(doc).unwrap().report().unwrap();
    assert!(imported.methods.contains("No contributing"));
    assert!(!paths.state.exists());
    assert!(!provenance::companion(&file).exists());
    session.apply(doc, edit()).unwrap();
    let edited = session.document_record(doc).unwrap();
    assert!(edited.unsaved);
    assert_eq!(edited.effective.len(), 1);
    assert!(edited.report().unwrap().methods.contains("Replaced"));
    session.undo(doc).unwrap();
    let undone = session.document_record(doc).unwrap();
    assert!(undone.effective.is_empty());
    assert!(!undone.unsaved);
    assert!(
        undone
            .report()
            .unwrap()
            .history
            .iter()
            .any(|row| row.status == "Undone / not contributing")
    );
    session.redo(doc).unwrap();
    assert_eq!(session.document_record(doc).unwrap().effective.len(), 1);
    session.save(doc, None).unwrap();
    assert!(!session.document_record(doc).unwrap().unsaved);
    assert_eq!(session.document_record(doc).unwrap().effective.len(), 1);
}

#[test]
fn reopened_ancestry_survives_multiple_saves_and_changed_disk_records() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let paths = paths(dir.path());
    let mut first = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let doc = first.open(&file).unwrap();
    first.apply(doc, edit()).unwrap();
    first.save(doc, None).unwrap();
    let mut next = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let doc = next.open(&file).unwrap();
    next.apply(
        doc,
        Op::SeqReplace {
            start: 2,
            end: 3,
            residues: "A".into(),
        },
    )
    .unwrap();
    next.save(doc, None).unwrap();
    next.apply(
        doc,
        Op::SeqReplace {
            start: 3,
            end: 4,
            residues: "C".into(),
        },
    )
    .unwrap();
    next.save(doc, None).unwrap();
    assert_eq!(provenance::inspect(&file).unwrap().effective_edits.len(), 3);
    assert_eq!(provenance::verify(&file).unwrap(), Vec::<String>::new());
    let record = next.document_record(doc).unwrap();
    std::fs::write(
        provenance::companion(&file).join("manifest.json"),
        "damaged",
    )
    .unwrap();
    assert_eq!(
        record.report().unwrap().methods,
        next.document_record(doc).unwrap().report().unwrap().methods
    );
    let mut fresh = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let fresh_doc = fresh.open(&file).unwrap();
    assert!(
        fresh
            .document_record(fresh_doc)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("could not be read"))
    );
}

#[test]
fn analysis_uses_submission_record_even_after_source_undo() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    let output = dir.path().join("result.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let paths = paths(dir.path());
    let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    let frozen = session.document_record(doc).unwrap();
    let bytes = b">gene\nATGT\n";
    let hash = session.snapshot_bytes(bytes).unwrap();
    std::fs::write(&output, bytes).unwrap();
    session.undo(doc).unwrap();
    let inputs = vec![("gene.fasta".into(), hash)];
    session
        .log_tool_run(
            doc,
            basecraft_protocol::Actor::Tool {
                tool: "Example".into(),
                version: "1".into(),
            },
            ("example", "copy"),
            serde_json::json!({}),
            &inputs,
            std::slice::from_ref(&output),
        )
        .unwrap();
    session
        .publish_analysis_snapshot(doc, &output, &inputs, &frozen)
        .unwrap();
    let publication = provenance::inspect(&output).unwrap();
    assert_eq!(publication.effective_edits.len(), 2);
    assert_eq!(provenance::verify(&output).unwrap(), Vec::<String>::new());
    assert!(session.document_record(doc).unwrap().effective.is_empty());
}

#[test]
fn two_input_records_preserve_distinct_origins_and_export_frozen_evidence() {
    use basecraft_ops::document_record::DocumentRecord;
    let dir = tempfile::tempdir().unwrap();
    let paths = paths(dir.path());
    let mut records = Vec::new();
    for name in ["first.fasta", "second.fasta"] {
        let file = dir.path().join(name);
        std::fs::write(&file, ">gene\nACGT\n").unwrap();
        let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
        let doc = session.open(&file).unwrap();
        session.apply(doc, edit()).unwrap();
        session.save(doc, None).unwrap();
        records.push(DocumentRecord::from_file(&file).unwrap());
    }
    let mut record = records[0].clone();
    record.include_input(&records[1]);
    record.include_input(&records[1]);
    assert_eq!(record.effective.len(), 2);
    let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let doc = session.open(&dir.path().join("first.fasta")).unwrap();
    let output = dir.path().join("combined.fasta");
    let bytes = b">first\nATGT\n>second\nATGT\n";
    std::fs::write(&output, bytes).unwrap();
    let hash = session.snapshot_bytes(bytes).unwrap();
    let inputs = vec![("combined input".into(), hash)];
    session
        .log_tool_run(
            doc,
            basecraft_protocol::Actor::External,
            ("example", "combine"),
            serde_json::json!({}),
            &inputs,
            std::slice::from_ref(&output),
        )
        .unwrap();
    session
        .publish_analysis_snapshot(doc, &output, &inputs, &record)
        .unwrap();
    let publication = provenance::inspect(&output).unwrap();
    assert_eq!(publication.parents.len(), 2);
    assert_eq!(publication.effective_edits.len(), 3);
    assert_eq!(provenance::verify(&output).unwrap(), Vec::<String>::new());
    let report = record.report().unwrap();
    let export = dir.path().join("reports");
    std::fs::create_dir(&export).unwrap();
    std::fs::write(export.join("methods.md"), "My own methods").unwrap();
    report.export(&export).unwrap();
    assert_eq!(
        std::fs::read_to_string(export.join("methods.md")).unwrap(),
        "My own methods"
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(export.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["revision"], report.revision);
}

#[test]
fn provenance_works_with_recovery_disabled_and_preserves_frozen_reports() {
    use basecraft_platform::settings::{Change, SettingsStore};
    let dir = tempfile::tempdir().unwrap();
    let paths = paths(dir.path());
    SettingsStore::new(paths.preferences())
        .update(Change::Recovery(false))
        .unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    let frozen = session.document_record(doc).unwrap().report().unwrap();
    session.undo(doc).unwrap();
    frozen.export(&dir.path().join("report")).unwrap();
    assert!(
        std::fs::read_to_string(dir.path().join("report/methods.generated.md"))
            .unwrap()
            .contains("Replaced")
    );
    assert!(
        session
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .methods
            .contains("No contributing")
    );
    session.redo(doc).unwrap();
    session.save(doc, None).unwrap();
    assert_eq!(provenance::verify(&file).unwrap(), Vec::<String>::new());
    assert!(!paths.state.join("recovery").exists());
}

#[test]
fn methods_use_frozen_plugin_metadata_and_reject_unmatched_contribution() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    let output = dir.path().join("result.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    std::fs::write(&output, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    let snapshot = session.document_record(doc).unwrap();
    let input_hash = session.snapshot_file(&file).unwrap();
    let inputs = vec![("gene.fasta".into(), input_hash)];
    session.log_tool_run(doc, basecraft_protocol::Actor::Tool { tool:"RecordedTool".into(),version:"1.2".into() }, ("example", "analyze"), serde_json::json!({"_execution":{"methods_template":"example.v1","methods":{"description":"Sequences were processed","citations":["A recorded citation"]},"resolved_parameters":{"seed":42}}}), &inputs, std::slice::from_ref(&output)).unwrap();
    session
        .publish_analysis_snapshot(doc, &output, &inputs, &snapshot)
        .unwrap();
    let publication = provenance::inspect(&output).unwrap();
    let mut record = basecraft_ops::document_record::DocumentRecord::from_publication(&publication);
    let report = record.report().unwrap();
    assert!(report.methods.contains("RecordedTool (version 1.2)"));
    assert!(report.methods.contains("seed = 42"));
    assert_eq!(report.citations, vec!["A recorded citation"]);
    record.effective[0].id = "missing".into();
    assert!(record.report().is_err());
}

#[test]
fn partial_save_as_can_retry_its_verified_output() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let target = dir.path().join("copy.fasta");
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let id = session.open(&file).unwrap();
    session.apply(id, edit()).unwrap();
    let bundle = provenance::companion(&target);
    std::fs::create_dir(&bundle).unwrap();
    std::fs::write(bundle.join("artifacts"), b"blocked").unwrap();
    assert!(
        session
            .save_new(id, Some(&target))
            .unwrap_err()
            .to_string()
            .contains("data saved")
    );
    std::fs::remove_file(bundle.join("artifacts")).unwrap();
    session.save_new(id, Some(&target)).unwrap();
    assert!(std::fs::read_to_string(target).unwrap().contains("ATGT"));
}
