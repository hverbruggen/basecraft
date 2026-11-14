use basecraft_ops::records::ReportExt;
use basecraft_ops::{Session, log, records};
use basecraft_protocol::Op;
use std::path::Path;

fn paths(root: &Path) -> basecraft_platform::paths::AppPaths {
    basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| root.join("profile").into_os_string())
    })
    .unwrap()
}
fn edit() -> Op {
    Op::seq_replace(1, 2, "T")
}

#[test]
fn passive_open_close_and_unchanged_save_create_no_records() {
    crate::isolated_profile::isolate!();
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
    assert!(!records::companion(&file).exists());
}

#[test]
fn edit_undo_redo_save_move_verify_and_report() {
    crate::isolated_profile::isolate!();
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
        assert_eq!(log::read_all(&recovery).unwrap().len(), 2);
        assert!(recovery.starts_with(dir.path().join(".basecraft/recovery")));
        session.undo(doc).unwrap();
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
        assert_eq!(records::verify(&file, None).unwrap(), Vec::<String>::new());
        let publication = records::inspect(&file, None).unwrap();
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
    records::package(&file, &exported, None).unwrap();
    let moved = exported.join("gene.fasta");
    std::fs::remove_file(&file).unwrap();
    std::fs::remove_dir_all(records::companion(&file)).unwrap();
    assert_eq!(records::verify(&moved, None).unwrap(), Vec::<String>::new());
    let reports = records::companion(&moved).join("reports");
    assert!(
        std::fs::read_to_string(reports.join("methods.generated.md"))
            .unwrap()
            .contains("Replaced")
    );
    std::fs::write(reports.join("methods.md"), "My reviewed prose").unwrap();
    records::write_reports(
        &records::companion(&moved),
        &records::inspect(&moved, None).unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(reports.join("methods.md")).unwrap(),
        "My reviewed prose"
    );
}

#[test]
fn completely_undone_edit_publishes_nothing_and_cleans_recovery() {
    crate::isolated_profile::isolate!();
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
    assert!(!records::companion(&file).exists());
    assert!(!recovery.exists());
}

#[test]
fn changed_source_refused_before_mutation_and_before_save() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    std::fs::write(&file, ">gene\nGGGG\n").unwrap();
    assert_eq!(
        session.apply(doc, edit()).unwrap_err().code(),
        "changed_on_disk"
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
fn history_is_fingerprints_only_and_path_escape_is_detected() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    session.save(doc, None).unwrap();
    let publication = records::inspect(&file, None).unwrap();
    assert!(
        publication
            .inputs
            .iter()
            .chain(&publication.outputs)
            .all(|artifact| artifact.retained.is_none())
    );
    assert!(
        std::fs::read_dir(records::companion(&file).join("artifacts"))
            .map_or(true, |mut retained| retained.next().is_none())
    );
    assert_eq!(
        records::verify(&file, None).unwrap(),
        [] as [std::string::String; 0]
    );
    let mut publication = publication;
    publication.inputs[0].retained = Some("../gene.fasta".into());
    std::fs::write(
        records::companion(&file).join("manifest.json"),
        basecraft_protocol::records::encode(&publication).unwrap(),
    )
    .unwrap();
    assert!(
        records::verify(&file, None)
            .unwrap()
            .iter()
            .any(|e| e.contains("unsafe"))
    );
}

#[test]
fn sidecar_failure_reports_partial_save_and_keeps_pending_recovery() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    // A directory at the companion location injects a publication write error.
    let bundle = records::companion(&file);
    std::fs::create_dir_all(&bundle).unwrap();
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

/// History has no off switch: without recovery, nothing goes to the private
/// folders, and the history still goes beside the file.
#[test]
fn without_recovery_history_still_goes_beside_the_file_and_nothing_private() {
    use basecraft_platform::settings::{Change, SettingsStore};
    crate::isolated_profile::isolate!();
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
    session.save(doc, None).unwrap();
    assert!(!paths.state.join("recovery").exists());
    assert!(!paths.state.join("provenance").exists());
    assert!(records::companion(&file).join("manifest.json").is_file());
}

#[test]
fn save_as_does_not_disable_original_conflict_detection() {
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
    let published = records::inspect(&file, None).unwrap();
    assert_eq!(published.outputs.len(), 2);
    let package = dir.path().join("package");
    records::package(&file, &package, None).unwrap();
    // The companion is published by name and kept where companions live:
    // the package's own .basecraft, and the original's.
    let companion = basecraft_ops::files::companion_path(None, &file);
    assert_eq!(
        companion.file_name().unwrap().to_string_lossy(),
        published.outputs[1].logical_name
    );
    assert!(basecraft_ops::files::companion_path(None, &package.join("tree.nwk")).is_file());
    std::fs::write(companion, "{}").unwrap();
    assert_ne!(records::verify(&file, None).unwrap(), Vec::<String>::new());
}

#[test]
fn journal_checksum_and_torn_tail_expose_only_verified_prefix() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
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
fn unsaved_recovery_reconstructs_exact_state_without_cache() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let recovery;
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
        recovery = session.recovery_root().to_path_buf();
        let doc = session.open(&file).unwrap();
        session.apply(doc, edit()).unwrap();
        session.apply(doc, Op::seq_replace(2, 3, "A")).unwrap();
        session.undo(doc).unwrap();
    }
    assert!(recovery.is_dir());
    let destination = dir.path().join("restored");
    let restored = basecraft_ops::records::recover(&recovery, &destination, None).unwrap();
    assert_eq!(restored, vec![destination.join("gene.fasta")]);
    assert_eq!(
        std::fs::read_to_string(&restored[0]).unwrap(),
        ">gene\nATGT\n"
    );
    assert!(basecraft_ops::records::recover(&recovery, &destination, None).is_err());
}

#[test]
fn reopen_and_save_as_preserve_contributing_ancestry() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
        let doc = session.open(&file).unwrap();
        session.apply(doc, edit()).unwrap();
        session.save(doc, None).unwrap();
    }
    let first = records::inspect(&file, None).unwrap();
    let copy = dir.path().join("copy.fasta");
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
        let doc = session.open(&file).unwrap();
        session.apply(doc, Op::seq_replace(2, 3, "A")).unwrap();
        session.save(doc, Some(&copy)).unwrap();
    }
    let publication = records::inspect(&copy, None).unwrap();
    assert_eq!(publication.parents, vec![first.id]);
    assert_eq!(publication.effective_edits.len(), 2);
    std::fs::remove_dir_all(records::companion(&file)).unwrap();
    assert_eq!(records::verify(&copy, None).unwrap(), Vec::<String>::new());
}

#[test]
fn recovery_rejects_traversal_and_skips_corrupt_retained_candidates() {
    crate::isolated_profile::isolate!();
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
                let writer = value.as_object_mut().unwrap().remove("basecraft").unwrap();
                if index == 0 {
                    value["params"]["path"] = open.into();
                    value["doc"] = key.into();
                }
                let checksum = basecraft_io::hash_bytes(&serde_json::to_vec(&value).unwrap());
                value["_checksum"] = checksum.into();
                value["_sequence"] = index.into();
                value["basecraft"] = writer;
                serde_json::to_string(&value).unwrap() + "\n"
            })
            .collect::<String>();
        std::fs::write(&journal, mutated).unwrap();
        assert!(
            basecraft_ops::records::recover(recovery, &dir.path().join("output"), None)
                .unwrap_err()
                .to_string()
                .contains("unsafe")
        );
        assert!(!dir.path().join("outside.fasta").exists());
    }
    std::fs::write(&journal, original).unwrap();
    let baseline = basecraft_ops::log::read_all(recovery).unwrap();
    let digest = baseline[0].result.hashes[0].1.trim_start_matches("b3:");
    std::fs::create_dir_all(basecraft_ops::session::objects_dir(recovery)).unwrap();
    std::fs::write(
        basecraft_ops::session::objects_dir(recovery).join(digest),
        "corrupt",
    )
    .unwrap();
    let outputs =
        basecraft_ops::records::recover(recovery, &dir.path().join("output"), None).unwrap();
    assert_eq!(std::fs::read(&outputs[0]).unwrap(), b">gene\nATGT\n");
}

#[test]
fn ancestor_only_artifacts_and_duplicate_parent_references_are_checked() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.apply(doc, edit()).unwrap();
    session.save(doc, None).unwrap();
    let ancestor = records::inspect(&file, None).unwrap();
    session.apply(doc, Op::seq_replace(2, 3, "A")).unwrap();
    session.save(doc, None).unwrap();
    let bundle = records::companion(&file);
    assert!(
        ancestor
            .outputs
            .iter()
            .all(|artifact| artifact.retained.is_none())
    );
    assert_eq!(
        records::verify(&file, None).unwrap(),
        [] as [std::string::String; 0]
    );
    let mut current = records::inspect(&file, None).unwrap();
    current.parents.push(current.parents[0].clone());
    let bytes = basecraft_protocol::records::encode(&current).unwrap();
    std::fs::write(bundle.join("manifest.json"), &bytes).unwrap();
    std::fs::write(
        bundle.join("records").join(format!("{}.json", current.id)),
        &bytes,
    )
    .unwrap();
    assert!(
        records::verify(&file, None)
            .unwrap_err()
            .to_string()
            .contains("duplicate parent")
    );
}

#[test]
fn current_record_tracks_unsaved_undo_redo_and_is_read_only() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let paths = paths(dir.path());
    let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
    let doc = session.open(&file).unwrap();
    let imported = session.document_record(doc).unwrap().report().unwrap();
    assert!(imported.methods.contains("No contributing"));
    assert!(!paths.state.exists());
    assert!(!records::companion(&file).exists());
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
    crate::isolated_profile::isolate!();
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
    next.apply(doc, Op::seq_replace(2, 3, "A")).unwrap();
    next.save(doc, None).unwrap();
    next.apply(doc, Op::seq_replace(3, 4, "C")).unwrap();
    next.save(doc, None).unwrap();
    assert_eq!(
        records::inspect(&file, None).unwrap().effective_edits.len(),
        3
    );
    assert_eq!(records::verify(&file, None).unwrap(), Vec::<String>::new());
    let record = next.document_record(doc).unwrap();
    std::fs::write(records::companion(&file).join("manifest.json"), "damaged").unwrap();
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
fn provenance_works_with_recovery_disabled_and_preserves_frozen_reports() {
    use basecraft_platform::settings::{Change, SettingsStore};
    crate::isolated_profile::isolate!();
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
    assert_eq!(
        records::verify(&file, None).unwrap(),
        [] as [std::string::String; 0]
    );
    assert!(
        !records::inspect(&file, None)
            .unwrap()
            .completeness
            .input_bytes_available
    );
    assert!(!paths.state.join("recovery").exists());
}

#[test]
fn partial_save_as_can_retry_its_verified_output() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let target = dir.path().join("copy.fasta");
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let id = session.open(&file).unwrap();
    session.apply(id, edit()).unwrap();
    let bundle = records::companion(&target);
    std::fs::create_dir_all(&bundle).unwrap();
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

#[test]
fn typing_bursts_undo_together_and_every_keystroke_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    let other = dir.path().join("other.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    std::fs::write(&other, ">other\nAAAA\n").unwrap();
    let insert = |at: u64, residues: &str| Op::seq_replace(at, at, residues);
    let recovery;
    {
        let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
        recovery = session.recovery_root().to_path_buf();
        let doc = session.open(&file).unwrap();
        let second = session.open(&other).unwrap();
        session.apply(doc, insert(4, "T")).unwrap();
        session.apply(doc, insert(5, "T")).unwrap();
        // A cursor jump starts a new burst.
        session.apply(doc, insert(0, "G")).unwrap();
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).unwrap().read(0, 6), b"ACGTTT");
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).unwrap().read(0, 4), b"ACGT");
        session.redo(doc).unwrap();
        assert_eq!(session.doc(doc).unwrap().read(0, 6), b"ACGTTT");
        // Typing in another document ends the burst, even where it continues.
        session.apply(doc, insert(6, "C")).unwrap();
        session.apply(second, insert(4, "C")).unwrap();
        session.apply(doc, insert(7, "C")).unwrap();
        session.undo(doc).unwrap();
        assert_eq!(session.doc(doc).unwrap().read(0, 7), b"ACGTTTC");
    }
    let inserts = log::read_all(&recovery)
        .unwrap()
        .into_iter()
        .filter(|entry| matches!(&entry.op, Op::SeqReplace { start, end, .. } if start == end))
        .count();
    assert_eq!(inserts, 6, "each keystroke is journaled");
    let destination = dir.path().join("restored");
    let restored = records::recover(&recovery, &destination, None).unwrap();
    let gene = restored.iter().find(|p| p.ends_with("gene.fasta")).unwrap();
    assert_eq!(std::fs::read_to_string(gene).unwrap(), ">gene\nACGTTTC\n");
}

#[test]
fn undo_reaches_a_fixed_depth_and_settled_edits_stay_in_effect() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&file).unwrap();
    let edits = 12_600;
    for i in 0..edits {
        let residue = ["C", "G"][i % 2];
        session.apply(doc, Op::seq_replace(0, 1, residue)).unwrap();
    }
    // Every edit stays in the record, undoable or not.
    assert_eq!(session.document_record(doc).unwrap().effective.len(), edits);
    let mut undone = 0;
    while session.undo(doc).is_ok() {
        undone += 1;
    }
    assert!((10_000..edits).contains(&undone), "{undone}");
    // Back at the oldest undoable step, the document still differs from disk.
    assert!(session.document_record(doc).unwrap().unsaved);
}
