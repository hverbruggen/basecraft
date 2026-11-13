//! Analysis index beside the files analyses used. Each folder holding such a
//! file keeps `.basecraft/analysis-index/`, one file per analysis store whose
//! runs depend on files there. Writers only add entries, before they change a
//! ledger, so the index may list more than is current, never less. Readers take
//! the union of every file in that folder, copies a sync tool made included, so
//! no writer's entry is lost to another writer or another machine. Each store
//! lists the folders it is indexed in, so a lost index is noticed and rebuilt.
use super::{Path, PathBuf, Result, lexical, stamp};
use basecraft_protocol::records::{AnalysisIndex, IndexedFolders};
use fs4::fs_std::FileExt;
use std::collections::{BTreeMap, BTreeSet};

/// The folder whose `.basecraft` indexes `source`, and the entry naming it
/// there. Every index read and write goes through here, so placing the index
/// elsewhere (for example at a project's root) changes only this function.
fn home(source: &Path) -> Option<(PathBuf, String)> {
    let name = source.file_name()?.to_str()?;
    Some((source.parent()?.into(), name.into()))
}

fn shards(folder: &Path) -> PathBuf {
    folder.join(".basecraft").join("analysis-index")
}

/// Every copy of a record in `dir` whose name starts with `stem`: the record
/// itself and any copy a sync tool made on a conflict. Locks and temporary
/// files are left out.
fn copies(dir: &Path, stem: &str) -> Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error.into()),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(stem)
            && !name.starts_with('.')
            && Path::new(&name).extension().is_none_or(|ext| ext != "lock")
            && entry.file_type()?.is_file()
        {
            if found.len() >= 4096 {
                return Err(crate::Error::Limit {
                    what: "analysis index files".into(),
                    limit: 4096,
                });
            }
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

fn read<T: serde::de::DeserializeOwned>(path: &Path, version: fn(&T) -> u32) -> Result<T> {
    let bytes = std::fs::read(path)?;
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::record_read(path, bytes.len() as u64);
    let record: T = serde_json::from_slice(&bytes).map_err(|e| crate::Error::Damaged {
        path: path.into(),
        detail: e.to_string(),
    })?;
    if version(&record) != 1 {
        return Err(crate::Error::Unsupported("analysis index version".into()));
    }
    Ok(record)
}

fn write(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
    // The entry must be durable before the ledger it covers changes.
    basecraft_platform::files::sync_directory(path.parent().unwrap())?;
    Ok(())
}

/// Create `path` and its missing parents, syncing each new entry.
fn folder(path: &Path) -> Result<()> {
    let missing: Vec<_> = path.ancestors().take_while(|p| !p.exists()).collect();
    std::fs::create_dir_all(path)?;
    for created in missing.into_iter().rev() {
        basecraft_platform::files::sync_directory(created.parent().unwrap())?;
    }
    Ok(())
}

/// A short lock beside the record, held only for one read and write, never
/// while waiting for anything else. It sits in the shared folder, so writers
/// with different profiles or user accounts exclude each other too.
fn lock_beside(path: &Path) -> Result<std::fs::File> {
    let lock = super::open_lock(path)?;
    lock.lock_exclusive()
        .map_err(|e| crate::Error::Busy(e.to_string()))?;
    Ok(lock)
}

/// The recorded spelling and, when it resolves, the canonical path. A folder
/// reached through a symbolic link is then found by either spelling.
pub(super) fn spellings(path: &Path) -> Vec<PathBuf> {
    let recorded = lexical(path);
    let mut all = vec![recorded.clone()];
    if let Ok(canonical) = recorded.canonicalize()
        && canonical != recorded
    {
        all.push(canonical);
    }
    all
}

/// Recovery staging is never a store a user's file depends on: it lives in
/// basecraft's own folders, a `.basecraft/recovery` folder or a recovery
/// attempt's `.basecraft-recover-*` folder.
pub(super) fn is_private(store: &Path) -> bool {
    let mut parts = store.components().map(std::path::Component::as_os_str);
    while let Some(part) = parts.next() {
        if part.to_string_lossy().starts_with(".basecraft-recover-")
            || part == ".basecraft" && parts.clone().next().is_some_and(|n| n == "recovery")
        {
            return true;
        }
    }
    let Ok(paths) = basecraft_platform::paths::AppPaths::discover() else {
        return false;
    };
    let store = store.canonicalize().unwrap_or_else(|_| lexical(store));
    [paths.state, paths.cache]
        .iter()
        .any(|root| store.starts_with(root.canonicalize().unwrap_or_else(|_| lexical(root))))
}

/// The store a ledger path `<store>/runs/<run>/input.json` belongs to.
pub(super) fn store_of(ledger: &Path) -> Option<&Path> {
    let runs = ledger.parent()?.parent()?;
    let store = runs.parent()?;
    (runs.file_name()? == "runs" && store.file_name()? == "analyses-v1").then_some(store)
}

/// The folder holding a store's `.basecraft`.
fn store_home(store: &Path) -> &Path {
    store.parent().and_then(Path::parent).unwrap_or(store)
}

/// The index file `folder` keeps for `store`, and the store's path as the
/// file records it. Named by that path, so a copied store (which keeps its
/// identity) gets its own file.
fn shard(folder: &Path, store: &Path) -> Result<(PathBuf, String)> {
    let relative = super::relative(folder, store)?.replace('\\', "/");
    let key = basecraft_io::hash_bytes(relative.as_bytes());
    let name = format!("{}.json", &key.trim_start_matches("b3:")[..32]);
    Ok((shards(folder).join(name), relative))
}

/// List `store` in the index beside each of `sources`. The caller holds the
/// sources' reservations, so no save of them is choosing stores meanwhile.
pub(super) fn add(store: &Path, sources: &BTreeSet<PathBuf>) -> Result<()> {
    record(store, sources, false).map(|_| ())
}

/// Rebuild `store`'s entries from `sources`: its file in each folder and its
/// list of folders are replaced, and its file in folders no longer used is
/// removed. Copies a sync tool made are left; they can only add entries.
pub(super) fn rebuild(store: &Path, sources: &BTreeSet<PathBuf>) -> Result<usize> {
    let store = std::path::absolute(store)?;
    let before = indexed_folders(&store)?;
    let after = record(&store, sources, true)?;
    for gone in before.difference(&after) {
        match std::fs::remove_file(shard(gone, &store)?.0) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.into());
            }
            _ => {}
        }
    }
    Ok(after.len())
}

fn record(store: &Path, sources: &BTreeSet<PathBuf>, replace: bool) -> Result<BTreeSet<PathBuf>> {
    if is_private(store) {
        return Ok(BTreeSet::new());
    }
    let store = std::path::absolute(store)?;
    let mut folders = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for source in sources.iter().filter(|source| !is_private(source)) {
        if let Some((folder, entry)) = home(source) {
            folders.entry(folder).or_default().insert(entry);
        }
    }
    let mut indexed = BTreeSet::new();
    for (folder, entries) in folders {
        if update(&folder, &store, &entries, replace)? {
            indexed.insert(folder);
        }
    }
    list_folders(&store, &indexed, replace)?;
    Ok(indexed)
}

/// Write this store's file in `folder`'s index. False when the folder itself
/// refuses new files: basecraft cannot save over a file there either.
fn update(
    folder_path: &Path,
    store: &Path,
    entries: &BTreeSet<String>,
    replace: bool,
) -> Result<bool> {
    let unwritable = |detail: String| {
        crate::error::invalid(format!(
            "cannot record which analyses use files in {}: {detail}",
            folder_path.display()
        ))
    };
    let hidden = folder_path.join(".basecraft");
    if std::fs::symlink_metadata(&hidden).is_ok_and(|m| !m.is_dir()) {
        return Err(unwritable(
            "its .basecraft is a file that belongs to another program".into(),
        ));
    }
    let result = (|| -> Result<()> {
        let dir = shards(folder_path);
        folder(&dir)?;
        let canonical = folder_path.canonicalize()?;
        let _lock = lock_beside(&canonical.join(".basecraft/analysis-index.lock"))?;
        let (path, relative) = shard(folder_path, store)?;
        let mut index = match read(&path, |i: &AnalysisIndex| i.schema_version) {
            Ok(index) if !replace => index,
            Err(crate::Error::Io(error)) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.into());
            }
            Err(error @ (crate::Error::Damaged { .. } | crate::Error::Unsupported(_)))
                if !replace =>
            {
                return Err(error);
            }
            _ => AnalysisIndex {
                schema_version: 1,
                store: relative.clone(),
                files: BTreeSet::new(),
            },
        };
        let before = index.clone();
        index.files.extend(entries.iter().cloned());
        if index != before || replace {
            write(&path, &index)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(true),
        Err(crate::Error::Io(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            if tempfile::tempfile_in(folder_path).is_err() {
                Ok(false)
            } else {
                Err(unwritable(error.to_string()))
            }
        }
        Err(crate::Error::Io(error)) => Err(unwritable(error.to_string())),
        Err(error) => Err(error),
    }
}

/// The folders whose index lists `store`, from every copy of its list.
fn indexed_folders(store: &Path) -> Result<BTreeSet<PathBuf>> {
    let home = store_home(store);
    let mut folders = BTreeSet::new();
    for path in copies(store, "indexed-folders")? {
        let list = read(&path, |l: &IndexedFolders| l.schema_version)?;
        folders.extend(list.folders.iter().map(|f| lexical(&home.join(f))));
    }
    Ok(folders)
}

fn list_folders(store: &Path, indexed: &BTreeSet<PathBuf>, replace: bool) -> Result<()> {
    let home = store_home(store);
    let _lock = lock_beside(&store.join("indexed-folders.lock"))?;
    let path = store.join("indexed-folders.json");
    let mut list = match read(&path, |l: &IndexedFolders| l.schema_version) {
        Ok(list) if !replace => list,
        Err(crate::Error::Io(error)) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(error.into());
        }
        Err(error @ (crate::Error::Damaged { .. } | crate::Error::Unsupported(_))) if !replace => {
            return Err(error);
        }
        _ => IndexedFolders {
            schema_version: 1,
            folders: BTreeSet::new(),
        },
    };
    let before = list.clone();
    if replace {
        list.folders.clear();
    }
    for folder in indexed {
        list.folders
            .insert(super::relative(home, folder)?.replace('\\', "/"));
    }
    if list != before || replace {
        write(&path, &list)?;
    }
    Ok(())
}

/// The index files beside `folder`, each with the store it names.
fn stores_beside(
    folder: &Path,
    inspection: &mut super::Inspection,
) -> Vec<(PathBuf, AnalysisIndex)> {
    let found = match copies(&shards(folder), "") {
        Ok(found) => found,
        Err(error) => {
            inspection.issue(
                super::InspectionCode::RegistryUnavailable,
                &shards(folder),
                error,
            );
            return Vec::new();
        }
    };
    let mut indexes = Vec::new();
    for path in found {
        match read(&path, |i: &AnalysisIndex| i.schema_version) {
            Ok(index) => indexes.push((lexical(&folder.join(&index.store)), index)),
            Err(error) => {
                inspection.issue(super::InspectionCode::RegistryUnavailable, &path, error);
            }
        }
    }
    indexes
}

/// The stores listed for `source` in the index beside it.
pub(super) fn listed(source: &Path) -> Result<Vec<PathBuf>> {
    let Some((folder, entry)) = home(&lexical(source)) else {
        return Ok(Vec::new());
    };
    let mut stores = Vec::new();
    for path in copies(&shards(&folder), "")? {
        let index = read(&path, |i: &AnalysisIndex| i.schema_version)?;
        if index.files.contains(&entry) {
            stores.push(lexical(&folder.join(&index.store)));
        }
    }
    Ok(stores)
}

/// Stores that can refer to `target`: those listed for it in the index beside
/// it under either spelling, and, for a file with several hard links, those
/// listed for any file there that is the same file. Stores that no longer
/// exist are left out: their ledgers went with them. With the session's
/// `project`, a store at the project's root that names the folder while the
/// index there does not name the store means the index was lost: the store is
/// inspected and the check is incomplete, so the save asks.
pub(super) fn select(
    target: &Path,
    absolute: &Path,
    project: Option<&Path>,
    inspection: &mut super::Inspection,
) -> Vec<PathBuf> {
    let linked = basecraft_platform::files::link_count(target).unwrap_or(1) > 1;
    let identity = stamp(target).ok().and_then(|s| Some((s.device?, s.inode?)));
    let same_file = |path: &Path| {
        identity.is_some() && stamp(path).ok().and_then(|s| Some((s.device?, s.inode?))) == identity
    };
    let mut selected = BTreeSet::new();
    let spellings: BTreeSet<_> = [target.to_path_buf(), lexical(absolute)].into();
    for spelling in spellings {
        let Some((folder, entry)) = home(&spelling) else {
            continue;
        };
        let hidden = folder.join(".basecraft");
        if std::fs::symlink_metadata(&hidden).is_ok_and(|m| !m.is_dir()) {
            inspection.issue(
                super::InspectionCode::RegistryUnavailable,
                &hidden,
                "this .basecraft belongs to another program; analyses of files here cannot be checked",
            );
            continue;
        }
        let indexes = stores_beside(&folder, inspection);
        for (store, index) in &indexes {
            if index.files.contains(&entry)
                || linked && index.files.iter().any(|f| same_file(&folder.join(f)))
            {
                selected.insert(store.clone());
            }
        }
        if let Some(project) = project {
            let store = project.join(".basecraft/analyses-v1");
            let named = indexes.iter().any(|(listed, _)| listed == &store);
            if !named && store.is_dir() {
                match indexed_folders(&store) {
                    Ok(folders) if folders.contains(&lexical(&folder)) => {
                        selected.insert(store);
                        inspection.issue(
                            super::InspectionCode::RegistryUnavailable,
                            &shards(&folder),
                            format!(
                                "the analysis index here is missing; `basecraft records reindex {}` rebuilds it",
                                project.display()
                            ),
                        );
                    }
                    Ok(_) => {}
                    Err(error) => inspection.issue(
                        super::InspectionCode::RegistryUnavailable,
                        &store.join("indexed-folders.json"),
                        error,
                    ),
                }
            }
        }
    }
    selected.retain(|store| store.try_exists().ok() != Some(false));
    selected.into_iter().collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn unrelated_save_during_pending_index_update() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.fasta");
        let other = root.path().join("other.fasta");
        let introduced = root.path().join("introduced.fasta");
        for path in [&source, &other, &introduced] {
            std::fs::write(path, b">a\nACGT\n").unwrap();
        }
        let run = super::super::reference_input_with_dependencies(
            &root.path().join("result.txt"),
            &source,
            &basecraft_io::hash_file(&source).unwrap(),
            &[],
            "pending update",
            None,
        )
        .unwrap();
        let locks = super::super::lock().unwrap();
        // A ledger writer reserves the ledger and every input it names.
        locks.record(run.directory()).unwrap();
        locks.source(&source).unwrap();
        locks.source(&introduced).unwrap();
        let store = run.directory().parent().unwrap().parent().unwrap();
        super::add(store, &[introduced.canonicalize().unwrap()].into()).unwrap();
        std::thread::spawn(move || {
            let mut session = crate::Session::new(other.parent().unwrap());
            let doc = session.open(&other).unwrap();
            session
                .apply(
                    doc,
                    basecraft_protocol::Op::SeqReplace {
                        start: 0,
                        end: 1,
                        residues: "T".into(),
                    },
                )
                .unwrap();
            session.save(doc, None).unwrap();
            assert_eq!(std::fs::read(&other).unwrap(), b">a\nTCGT\n");
            // Both the old index and the newly introduced source stay protected.
            for path in [source, introduced] {
                let doc = session.open(&path).unwrap();
                session
                    .apply(
                        doc,
                        basecraft_protocol::Op::SeqReplace {
                            start: 0,
                            end: 1,
                            residues: "T".into(),
                        },
                    )
                    .unwrap();
                assert!(matches!(
                    session.save(doc, None),
                    Err(crate::Error::Busy(_))
                ));
                assert_eq!(std::fs::read(path).unwrap(), b">a\nACGT\n");
            }
        })
        .join()
        .unwrap();
    }

    #[test]
    fn independent_save_overlaps_but_new_reference_and_alias_wait() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("first.fasta");
        let other = root.path().join("second.fasta");
        let alias = root.path().join("alias.fasta");
        std::fs::write(&target, b">a\nACGT\n").unwrap();
        std::fs::write(&other, b">b\nACGT\n").unwrap();
        std::fs::hard_link(&target, &alias).unwrap();
        let _first = super::super::lock_save(&target, false, &[], None).unwrap();
        std::thread::spawn(move || {
            let mut session = crate::Session::new(other.parent().unwrap());
            let id = session.open(&other).unwrap();
            session
                .apply(
                    id,
                    basecraft_protocol::Op::SeqReplace {
                        start: 0,
                        end: 1,
                        residues: "T".into(),
                    },
                )
                .unwrap();
            session.save(id, None).unwrap();
            for source in [target, alias] {
                let result = super::super::reference_input_with_dependencies(
                    &other,
                    &source,
                    &basecraft_io::hash_file(&source).unwrap(),
                    &[],
                    "concurrent reference",
                    None,
                );
                assert!(matches!(result, Err(crate::Error::Busy(_))));
            }
        })
        .join()
        .unwrap();
    }

    #[test]
    fn a_shared_hold_is_never_upgraded() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        let locks = super::super::lock().unwrap();
        locks.reserve([root.path().to_path_buf()], false).unwrap();
        assert!(matches!(
            locks.reserve([root.path().to_path_buf()], true),
            Err(crate::Error::Busy(_))
        ));
    }

    #[test]
    fn results_with_history_in_one_store_save_concurrently() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        // Two analysis results whose history bundles are runs of one store.
        for name in ["a.fasta", "b.fasta"] {
            let output = root.path().join(name);
            std::fs::write(&output, b">a\nACGT\n").unwrap();
            let run =
                crate::records::analysis_store::ArtifactRun::create(&output, &name[..1]).unwrap();
            run.bind(&output).unwrap();
        }
        let savers: Vec<_> = ["a.fasta", "b.fasta"]
            .into_iter()
            .map(|name| {
                let root = root.path().to_path_buf();
                std::thread::spawn(move || {
                    let mut session = crate::Session::recoverable(&root).unwrap();
                    let doc = session.open(&root.join(name)).unwrap();
                    session
                        .apply(
                            doc,
                            basecraft_protocol::Op::SeqReplace {
                                start: 0,
                                end: 1,
                                residues: "T".into(),
                            },
                        )
                        .unwrap();
                    session.save(doc, None)
                })
            })
            .collect();
        for saver in savers {
            saver.join().unwrap().unwrap();
        }
        for name in ["a.fasta", "b.fasta"] {
            assert_eq!(
                std::fs::read(root.path().join(name)).unwrap(),
                b">a\nTCGT\n"
            );
        }
    }
}
