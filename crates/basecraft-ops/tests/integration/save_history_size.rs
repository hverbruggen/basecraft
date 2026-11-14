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
        rows.push((save + 1, elapsed, written, total(&history), changed, lock_files()));
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
}
