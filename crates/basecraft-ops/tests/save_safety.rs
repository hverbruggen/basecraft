//! Save-path guards and failure injection (DATA-0007). Every test writes only
//! inside its own temporary folders and profile.

#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{Session, delimitation, log, replay};
use basecraft_protocol::{
    Op,
    delimitation::{ColumnStyle, DelimitationEdit, TreeDrawingEdit},
};
use std::io::Write;
use std::path::{Path, PathBuf};

const SPART: &str = include_str!("../../../testdata/fixtures/delimitation/manual.spart");

fn paths(root: &Path) -> basecraft_platform::paths::AppPaths {
    basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| root.join("profile").into_os_string())
    })
    .unwrap()
}

fn replace(start: u64, residues: &str) -> Op {
    Op::SeqReplace {
        start,
        end: start + residues.len() as u64,
        residues: residues.into(),
    }
}

fn listing(folder: &Path) -> Vec<PathBuf> {
    let mut names: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    names
}

#[test]
fn save_as_alias_of_own_file_keeps_stale_source_protection() {
    if isolated_profile::enter("save_as_alias_of_own_file_keeps_stale_source_protection").is_none()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    std::fs::write(&file, ">external\nAAAA\n").unwrap();
    let mut aliases = vec![dir.path().join("sub/../gene.fasta")];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path(), dir.path().join("folder-link")).unwrap();
        aliases.push(dir.path().join("folder-link/gene.fasta"));
    }
    for alias in aliases {
        let error = session.save(doc, Some(&alias)).unwrap_err().to_string();
        assert!(error.contains("source changed"), "{alias:?}: {error}");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            ">external\nAAAA\n",
            "{alias:?} replaced an external edit"
        );
    }
    assert!(session.can_undo(doc));

    // Unchanged on disk: the alias save is an ordinary save of the document.
    let other = dir.path().join("other.fasta");
    std::fs::write(&other, ">other\nACGT\n").unwrap();
    let doc = session.open(&other).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    session
        .save(doc, Some(&dir.path().join("sub/../other.fasta")))
        .unwrap();
    assert!(!session.document_record(doc).unwrap().unsaved);
    session.apply(doc, replace(2, "A")).unwrap();
    session.save(doc, None).unwrap();
    assert!(std::fs::read_to_string(&other).unwrap().contains("ATAT"));
}

#[test]
fn export_and_extract_refuse_an_open_documents_own_file() {
    if isolated_profile::enter("export_and_extract_refuse_an_open_documents_own_file").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGTACGT\n").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    for target in [file.clone(), dir.path().join("sub/../gene.fasta")] {
        let mut wrote = false;
        let error = session
            .export(doc, &target, "svg", |w| {
                wrote = true;
                w.write_all(b"<svg/>")
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("open document"), "{error}");
        assert!(!wrote);
        let error = session
            .extract(doc, (0, 4), &[], false, &target)
            .unwrap_err()
            .to_string();
        assert!(error.contains("open document"), "{error}");
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), ">gene\nACGTACGT\n");
    session
        .export(doc, &dir.path().join("figure.svg"), "svg", |w| {
            w.write_all(b"<svg/>")
        })
        .unwrap();
    session.save(doc, None).unwrap();
}

#[test]
fn failed_export_stream_keeps_previous_destination_and_records_nothing() {
    if isolated_profile::enter(
        "failed_export_stream_keeps_previous_destination_and_records_nothing",
    )
    .is_none()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let file = data.join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let figure = data.join("figure.svg");
    std::fs::write(&figure, "previous figure").unwrap();
    let before = listing(&data);
    let mut session = Session::recoverable_with_paths(&data, &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    // A full disk or failing renderer stops the stream part-way.
    let error = session
        .export(doc, &figure, "svg", |w| {
            w.write_all(b"<svg partial")?;
            Err(std::io::Error::from(std::io::ErrorKind::StorageFull))
        })
        .unwrap_err();
    assert_ne!(error.to_string(), "");
    assert_eq!(std::fs::read_to_string(&figure).unwrap(), "previous figure");
    assert_eq!(listing(&data), before, "temporary file left behind");
    let record = session.document_record(doc).unwrap();
    assert!(
        !record
            .events
            .iter()
            .any(|e| matches!(e.op, Op::Export { .. }))
    );
    assert!(record.unsaved);
}

#[cfg(unix)]
#[test]
fn read_only_folder_refuses_save_and_keeps_original_and_edits() {
    if isolated_profile::enter("read_only_folder_refuses_save_and_keeps_original_and_edits")
        .is_none()
    {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let file = data.join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(&data, &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    let before = listing(&data);
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::File::create(data.join("probe")).is_ok() {
        // Running with privileges that ignore permissions: nothing to inject.
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: permissions are not enforced for this user");
        return;
    }
    let error = session.save(doc, None).unwrap_err().to_string();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    eprintln!("read-only save error: {error}");
    assert!(error.to_lowercase().contains("permission"), "{error}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), ">gene\nACGT\n");
    assert_eq!(listing(&data), before);
    assert!(session.document_record(doc).unwrap().unsaved);
    assert!(session.can_undo(doc));
    session.save(doc, None).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("ATGT"));
}

/// Fill the file system holding `folder` until a write fails.
fn fill(folder: &Path) -> PathBuf {
    let filler = folder.join("filler");
    let mut file = std::fs::File::create(&filler).unwrap();
    let chunk = vec![0u8; 64 << 10];
    loop {
        if let Err(error) = file.write_all(&chunk).and_then(|()| file.sync_all()) {
            assert_eq!(error.raw_os_error(), Some(28), "{error}");
            break;
        }
    }
    filler
}

/// Tests filling the shared small file system take turns.
static FULL_DISK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Real ENOSPC on a small dedicated file system. The focused the build host runner
/// mounts one and sets this variable; without it there is nothing to fill.
fn full_disk_folder(name: &str) -> Option<PathBuf> {
    let root = std::env::var_os("BASECRAFT_TEST_FULL_DIR")?;
    let folder = Path::new(&root).join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    Some(folder)
}

#[test]
fn full_disk_save_keeps_original_and_retries_after_space_returns() {
    if isolated_profile::enter("full_disk_save_keeps_original_and_retries_after_space_returns")
        .is_none()
    {
        return;
    }
    let _turn = FULL_DISK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(data) = full_disk_folder("save") else {
        eprintln!("skipped: BASECRAFT_TEST_FULL_DIR is not set");
        return;
    };
    let profile = tempfile::tempdir().unwrap();
    let file = data.join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(&data, &paths(profile.path())).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    let filler = fill(&data);
    let before = listing(&data);
    let error = session.save(doc, None).unwrap_err().to_string();
    eprintln!("full-disk save error: {error}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), ">gene\nACGT\n");
    assert_eq!(listing(&data), before, "temporary file left behind");
    assert!(session.document_record(doc).unwrap().unsaved);
    std::fs::remove_file(filler).unwrap();
    session.save(doc, None).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("ATGT"));
    drop(session);
    std::fs::remove_dir_all(&data).unwrap();
}

#[test]
fn full_disk_journal_append_rolls_back_and_keeps_a_recoverable_prefix() {
    if isolated_profile::enter("full_disk_journal_append_rolls_back_and_keeps_a_recoverable_prefix")
        .is_none()
    {
        return;
    }
    let _turn = FULL_DISK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(full) = full_disk_folder("journal") else {
        eprintln!("skipped: BASECRAFT_TEST_FULL_DIR is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(&full)).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    let filler = fill(&full);
    let generation = session.generation(doc);
    // A long insertion needs more journal space than is left.
    let long = "A".repeat(256 << 10);
    let error = session.apply(
        doc,
        Op::SeqReplace {
            start: 4,
            end: 4,
            residues: long,
        },
    );
    eprintln!("full-disk journal error: {error:?}");
    assert!(error.is_err());
    assert_eq!(session.generation(doc), generation, "edit not rolled back");
    assert_eq!(session.doc(doc).len(), 4);
    let root = session.recovery_root().to_path_buf();
    assert!(!root.join("recovery-integrity-error.json").exists());
    let prefix = log::verified_session_prefix(&root).unwrap();
    assert!(prefix.problem.is_none(), "{:?}", prefix.problem);
    std::fs::remove_file(filler).unwrap();
    let recovered = dir.path().join("recovered");
    let outputs = replay::recover(&root, &recovered).unwrap();
    assert!(
        std::fs::read_to_string(&outputs[0])
            .unwrap()
            .contains("ATGT")
    );
    drop(session);
    std::fs::remove_dir_all(&full).unwrap();
}

fn journal(root: &Path) -> PathBuf {
    let host = listing(&log::log_dir(root)).remove(0);
    listing(&host).remove(0)
}

#[test]
fn damaged_journal_recovery_names_the_salvage_and_never_skips_events() {
    if isolated_profile::enter("damaged_journal_recovery_names_the_salvage_and_never_skips_events")
        .is_none()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let file = data.join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(&data, &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    session.apply(doc, replace(3, "A")).unwrap();
    session.flush().unwrap();
    let root = session.recovery_root().to_path_buf();
    // A crash: the process stops without saving.
    drop(session);
    let journal = journal(&root);
    let complete = std::fs::read(&journal).unwrap();

    // Torn last line, as left by a crash during an append.
    let mut torn = complete.clone();
    torn.extend_from_slice(b"{\"v\":1,\"id\":\"incomplete");
    std::fs::write(&journal, &torn).unwrap();
    let strict = dir.path().join("strict");
    let error = replay::recover(&root, &strict).unwrap_err().to_string();
    assert!(error.contains("--verified-prefix"), "{error}");
    assert!(error.contains("3 verified journal events"), "{error}");
    assert!(!strict.exists());
    let salvaged = dir.path().join("salvaged");
    let (outputs, problem) = replay::recover_verified_prefix(&root, &salvaged).unwrap();
    assert!(problem.unwrap().contains("torn"));
    assert!(
        std::fs::read_to_string(&outputs[0])
            .unwrap()
            .contains("ATGA")
    );
    assert_eq!(std::fs::read(&journal).unwrap(), torn, "journal changed");

    // A corrupt middle event stops the prefix; later events are not used.
    let text = String::from_utf8(complete).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    let damaged = lines[1].replacen("seq.replace", "seq.replacf", 1);
    lines[1] = &damaged;
    std::fs::write(&journal, format!("{}\n", lines.join("\n"))).unwrap();
    let partial = dir.path().join("partial");
    let (outputs, problem) = replay::recover_verified_prefix(&root, &partial).unwrap();
    assert!(problem.unwrap().contains("checksum"));
    assert!(
        outputs.is_empty()
            || !std::fs::read_to_string(&outputs[0])
                .unwrap()
                .contains("ATG")
    );
    assert!(std::fs::read_to_string(&file).unwrap().contains("ACGT"));
}

fn open_species(dir: &Path) -> (Session, basecraft_ops::DocId) {
    let path = dir.join("input.spart");
    std::fs::write(&path, SPART).unwrap();
    let mut session = Session::new(dir);
    let doc = session.open(&path).unwrap();
    (session, doc)
}

#[test]
fn analysis_result_hypotheses_and_columns_undo_as_one_step() {
    if isolated_profile::enter("analysis_result_hypotheses_and_columns_undo_as_one_step").is_none()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut session, doc) = open_species(dir.path());
    let initial = session.delimitation(doc).clone();
    let mut result = basecraft_io::spart::parse_text(SPART).unwrap();
    let template = result.hypotheses[0].clone();
    result.hypotheses = ["fitted-1", "fitted-2"]
        .into_iter()
        .map(|id| {
            let mut h = template.clone();
            h.id = id.into();
            h.name = id.into();
            h
        })
        .collect();
    let report = dir.path().join("result.json");
    std::fs::write(
        &report,
        serde_json::to_vec(&serde_json::json!({"workspace": result})).unwrap(),
    )
    .unwrap();
    delimitation::add_analysis_result(&mut session, doc, &report).unwrap();
    let added = session.delimitation(doc).clone();
    assert_eq!(added.hypotheses.len(), initial.hypotheses.len() + 2);
    assert!(added.view.visible.contains(&"fitted-2".to_string()));
    session.undo(doc).unwrap();
    assert_eq!(session.delimitation(doc), &initial, "one undo step");
    session.redo(doc).unwrap();
    assert_eq!(session.delimitation(doc), &added);
    session.save(doc, None).unwrap();
    drop(session);
    assert!(replay::replay(dir.path()).unwrap().ok());
}

#[test]
fn species_view_columns_and_tree_flips_survive_save_reopen_undo_and_replay() {
    if isolated_profile::enter(
        "species_view_columns_and_tree_flips_survive_save_reopen_undo_and_replay",
    )
    .is_none()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut session, doc) = open_species(dir.path());
    let path = dir.path().join("input.spart");
    let initial = session.delimitation(doc).view.clone();
    let mut view = initial.clone();
    view.visible = vec!["partition-2".into(), "partition-1".into()];
    view.columns.insert(
        "partition-1".into(),
        ColumnStyle {
            width: Some(12),
            gap: Some(3),
            spacing: None,
            colors: [("group-1".to_string(), "#aabbcc".to_string())].into(),
            names: true,
        },
    );
    view.tree_edits = vec![
        TreeDrawingEdit::Ladderize { small_first: true },
        TreeDrawingEdit::Rotate {
            clade: vec!["a".into(), "c".into()],
        },
    ];
    session
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::SetView { view: view.clone() }),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    let mut reopened = Session::unlogged(dir.path());
    let copy = reopened.open(&path).unwrap();
    assert_eq!(reopened.delimitation(copy).view, view);

    // Undo after saving returns to the earlier view, and that saves too.
    session.undo(doc).unwrap();
    assert_eq!(session.delimitation(doc).view, initial);
    assert!(session.document_record(doc).unwrap().unsaved);
    session.save(doc, None).unwrap();
    let mut reopened = Session::unlogged(dir.path());
    let copy = reopened.open(&path).unwrap();
    assert_eq!(reopened.delimitation(copy).view, initial);
    drop(session);
    let report = replay::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn report_export_refreshes_its_own_files_but_never_replaces_unrelated_ones() {
    if isolated_profile::enter(
        "report_export_refreshes_its_own_files_but_never_replaces_unrelated_ones",
    )
    .is_none()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, replace(1, "T")).unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    let folder = dir.path().join("dataset");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("manifest.json"), "{\"files\":[\"reads.fq\"]}").unwrap();
    std::fs::write(folder.join("history.md"), "Field notes").unwrap();
    let before = listing(&folder);
    let error = report.export(&folder).unwrap_err().to_string();
    assert!(error.contains("not from a Basecraft report"), "{error}");
    assert_eq!(listing(&folder), before);
    assert_eq!(
        std::fs::read_to_string(folder.join("history.md")).unwrap(),
        "Field notes"
    );
    // A new folder, then a refresh of that same export, both succeed.
    let export = dir.path().join("report");
    report.export(&export).unwrap();
    session.apply(doc, replace(2, "A")).unwrap();
    let newer = session.document_record(doc).unwrap().report().unwrap();
    newer.export(&export).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(export.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["revision"], newer.revision);
}

#[test]
fn interrupted_result_group_resumes_after_journal_failure_as_one_undo_step() {
    if isolated_profile::enter(
        "interrupted_result_group_resumes_after_journal_failure_as_one_undo_step",
    )
    .is_none()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut session, doc) = open_species(dir.path());
    let initial = session.delimitation(doc).clone();
    let mut result = basecraft_io::spart::parse_text(SPART).unwrap();
    let template = result.hypotheses[0].clone();
    result.hypotheses = ["repair-1", "repair-2"]
        .into_iter()
        .map(|id| {
            let mut h = template.clone();
            h.id = id.into();
            h.name = id.into();
            h
        })
        .collect();
    let report = dir.path().join("result.json");
    std::fs::write(
        &report,
        serde_json::to_vec(&serde_json::json!({"workspace": result})).unwrap(),
    )
    .unwrap();
    let (_, edits) = delimitation::analysis_result_edits(&session, doc, &report).unwrap();
    let op = |edit: &DelimitationEdit| Op::DelimitationEdit {
        edit: Box::new(edit.clone()),
    };
    let group = session.begin_group();
    session.apply(doc, op(&edits[0])).unwrap();
    let revision = session.revision(doc);
    // Force journal rotation, then make its parent unavailable for the second
    // attachment. This is a real append failure after the first edit committed.
    let host = listing(&log::log_dir(dir.path())).remove(0);
    let journal = listing(&host)
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .unwrap();
    let length = std::fs::metadata(&journal).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&journal)
        .unwrap()
        .set_len(8 << 20)
        .unwrap();
    let backup = dir.path().join("journal-backup");
    std::fs::rename(&host, &backup).unwrap();
    std::fs::write(&host, b"fault").unwrap();
    assert!(session.apply(doc, op(&edits[1])).is_err());
    session.end_group();
    assert_eq!(session.revision(doc), revision);
    std::fs::remove_file(&host).unwrap();
    std::fs::rename(&backup, &host).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&journal)
        .unwrap()
        .set_len(length)
        .unwrap();
    session.resume_group(doc, group).unwrap();
    for edit in &edits[1..] {
        session.apply(doc, op(edit)).unwrap();
    }
    session.end_group();
    let completed = session.delimitation(doc).clone();
    assert_eq!(completed.hypotheses.len(), initial.hypotheses.len() + 2);
    session.undo(doc).unwrap();
    assert_eq!(session.delimitation(doc), &initial);
    assert!(session.resume_group(doc, group).is_err());
    session.redo(doc).unwrap();
    assert_eq!(session.delimitation(doc), &completed);
    drop(session);
    let replay = replay::replay(dir.path()).unwrap();
    assert!(replay.ok(), "{:?}", replay.problems);
}
