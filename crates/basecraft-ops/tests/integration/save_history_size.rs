//! What repeated saves write: each history record holds only its own save's
//! events, so the files a save writes follow the edits made since the last
//! save, not every edit ever made (D24).
use basecraft_ops::{Session, project::Project};
use basecraft_protocol::Op;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Every file under `root` with its size and modification time.
fn files(root: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime)> {
    let mut found = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                folders.push(entry.path());
            } else {
                found.insert(entry.path(), (metadata.len(), metadata.modified().unwrap()));
            }
        }
    }
    found
}

fn total(root: &Path) -> u64 {
    if !root.exists() {
        return 0;
    }
    files(root).values().map(|(size, _)| size).sum()
}

#[test]
fn eight_saves_of_two_hundred_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let locks = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("locks");
    let lock_files = || std::fs::read_dir(&locks).map_or(0, Iterator::count);
    let folder = root.join("project");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("gene.fasta");
    let mut bytes = b">gene\n".to_vec();
    for _ in 0..200_000 / 60 {
        bytes.extend_from_slice(&[b'A'; 60]);
        bytes.push(b'\n');
    }
    std::fs::write(&file, &bytes).unwrap();
    let project = Project::new(&folder).unwrap();
    let mut session = Session::recoverable(&folder)
        .unwrap()
        .in_project(Some(project));
    let doc = session.open(&file).unwrap();
    let mut rows = Vec::new();
    for save in 0..8u64 {
        for edit in 0..200u64 {
            let at = save * 200 + edit;
            session
                .apply(doc, Op::seq_replace(at * 7, at * 7 + 1, "C"))
                .unwrap();
        }
        let before = files(&root);
        let started = Instant::now();
        session.save(doc, None).unwrap();
        let elapsed = started.elapsed();
        let after = files(&root);
        let mut written = 0;
        let mut changed = Vec::new();
        for (path, (size, modified)) in &after {
            match before.get(path) {
                Some((old, _)) if path.extension().is_some_and(|e| e == "jsonl") => {
                    written += size.saturating_sub(*old);
                }
                Some(old) if old == &(*size, *modified) => {}
                _ => {
                    written += size;
                    changed.push(path.strip_prefix(&root).unwrap().to_path_buf());
                }
            }
        }
        let history = folder.join(".basecraft/provenance");
        rows.push((
            save + 1,
            elapsed,
            written,
            total(&history),
            changed,
            lock_files(),
        ));
    }
    eprintln!("save | time | bytes written | history total | files replaced | lock files");
    for (save, elapsed, written, history, changed, locks) in &rows {
        eprintln!(
            "{save} | {} ms | {written} | {history} | {} | {locks}",
            elapsed.as_millis(),
            changed.len()
        );
    }
    for path in &rows.last().unwrap().4 {
        eprintln!("eighth save replaced {}", path.display());
    }
    // The eighth save writes the sequence, one history record, the pointer
    // to it and the recovery journal's new stream: no copies, no reports.
    let eighth = &rows.last().unwrap().4;
    let history: Vec<_> = eighth
        .iter()
        .filter(|p| p.starts_with("project/.basecraft/provenance"))
        .collect();
    assert_eq!(history.len(), 2, "{history:?}");
    assert!(history.iter().any(|p| p.ends_with("manifest.json")));
    assert!(
        history
            .iter()
            .any(|p| p.parent().unwrap().ends_with("records"))
    );
    // Bytes follow the edits since the last save, not every edit ever made.
    assert!(
        rows[7].2 < rows[1].2 + rows[1].2 / 10,
        "{} vs {}",
        rows[7].2,
        rows[1].2
    );
    let bundle = basecraft_ops::records::bundle_for_result(&file, session.project());
    assert_eq!(
        std::fs::read_dir(bundle.join("records")).unwrap().count(),
        8
    );
    let (_, history) = basecraft_ops::records::saved_history(&file, session.project()).unwrap();
    assert_eq!(history.effective.len(), 1600);
    assert!(history.gaps.is_empty());
}

fn edited_and_saved(folder: &Path, saves: u64) -> PathBuf {
    let file = folder.join("gene.fasta");
    std::fs::write(&file, ">gene\nACGTACGTACGT\n").unwrap();
    let mut session = Session::recoverable(folder).unwrap();
    let doc = session.open(&file).unwrap();
    for save in 0..saves {
        session
            .apply(doc, Op::seq_replace(save, save + 1, "T"))
            .unwrap();
        session.save(doc, None).unwrap();
    }
    file
}

#[test]
fn a_reopened_file_shows_its_whole_history_and_a_lost_record_as_a_gap() {
    let dir = tempfile::tempdir().unwrap();
    let file = edited_and_saved(dir.path(), 3);
    let bundle = basecraft_ops::records::bundle_for_result(&file, None);
    let latest = basecraft_ops::records::inspect(&file, None).unwrap();
    assert_eq!(latest.effective_edits.len(), 1);
    let mut session = Session::recoverable(dir.path()).unwrap();
    let doc = session.open(&file).unwrap();
    let record = session.document_record(doc).unwrap();
    assert_eq!(record.effective.len(), 3);
    assert!(record.warnings.iter().all(|w| !w.contains("incomplete")));
    // Lose the middle record: the newest is still read, the rest is a gap.
    let middle = &latest.parents[0];
    std::fs::remove_file(bundle.join("records").join(format!("{middle}.json"))).unwrap();
    let mut session = Session::recoverable(dir.path()).unwrap();
    let doc = session.open(&file).unwrap();
    let record = session.document_record(doc).unwrap();
    assert_eq!(record.effective.len(), 1);
    assert!(
        record
            .warnings
            .iter()
            .any(|w| w.contains(middle.as_str()) && w.contains("is missing")),
        "{:?}",
        record.warnings
    );
    let report = record.report().unwrap();
    assert_eq!(report.history[0].summary, "Earlier history missing");
    assert!(basecraft_ops::records::verify(&file, None).is_err());
}

#[test]
fn after_a_save_the_journal_holds_only_unsaved_edits() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGTACGT\n").unwrap();
    let mut session = Session::recoverable(dir.path()).unwrap();
    let doc = session.open(&file).unwrap();
    session.apply(doc, Op::seq_replace(0, 1, "T")).unwrap();
    session.apply(doc, Op::seq_replace(1, 2, "T")).unwrap();
    session.save(doc, None).unwrap();
    session.apply(doc, Op::seq_replace(2, 3, "A")).unwrap();
    let recovery = session.recovery_root().to_path_buf();
    let journal = basecraft_ops::log::read_all(&recovery).unwrap();
    let ops: Vec<_> = journal.iter().map(|e| e.op.name()).collect();
    assert!(matches!(journal[0].op, Op::Save { .. }), "{ops:?}");
    assert_eq!(journal.len(), 2, "{ops:?}");
    // A crash now: recovery starts at the save and replays the later edit.
    std::mem::forget(session);
    let restored =
        basecraft_ops::records::recover(&recovery, &dir.path().join("out"), None).unwrap();
    assert_eq!(std::fs::read(&restored[0]).unwrap(), b">gene\nTTATACGT\n");
}
