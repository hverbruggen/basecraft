//! The analysis index: which analysis stores have runs that depend on a file.
//! A project keeps one index in its own `.basecraft`, keyed by
//! project-relative path; a file outside the writing session's project is
//! indexed in a `.basecraft` beside it (rules 6.0 and 2.5). Entries sit in 256
//! buckets by a hash of the path, one file per store in each bucket, so a save
//! reads one small folder. Writers only add entries, before they change a
//! ledger, so the index may list more than is current, never less. Readers
//! take the union of every file in a bucket, copies a sync tool made included,
//! so no writer's entry is lost to another writer or machine. Each store lists
//! the buckets it is indexed in, so a lost index is noticed and rebuilt.
use super::{Path, PathBuf, Result, lexical, stamp};
use basecraft_protocol::records::{AnalysisIndex, IndexedFolders};
use fs4::fs_std::FileExt;
use std::collections::{BTreeMap, BTreeSet};

/// The folder whose `.basecraft` indexes `source`, and the entry naming it
/// there: the project's root and the project-relative path for a file inside
/// `project`, otherwise the file's own folder and its name.
fn home(source: &Path, project: Option<&Path>) -> Option<(PathBuf, String)> {
    if let Some(project) = project
        && let Some(rest) = basecraft_platform::files::inside(source, project)
    {
        return Some((project.into(), rest.to_str()?.replace('\\', "/")));
    }
    let name = source.file_name()?.to_str()?;
    Some((source.parent()?.into(), name.into()))
}

/// Why basecraft will not use `folder`'s `.basecraft`, when it is there but
/// is not an ordinary folder. A symbolic link is not followed.
pub(crate) fn foreign_basecraft(folder: &Path) -> Option<&'static str> {
    let metadata = std::fs::symlink_metadata(folder.join(".basecraft")).ok()?;
    if metadata.file_type().is_symlink() {
        Some("its .basecraft is a symbolic link, which basecraft does not follow")
    } else if metadata.is_dir() {
        None
    } else {
        Some("its .basecraft is a file that belongs to another program")
    }
}

fn index_root(home: &Path) -> PathBuf {
    home.join(".basecraft").join("analysis-index")
}

/// The bucket folder holding `entry`'s index files.
fn bucket(home: &Path, entry: &str) -> PathBuf {
    let key = basecraft_io::hash_bytes(entry.as_bytes());
    index_root(home).join(&key.trim_start_matches("b3:")[..2])
}

/// The folder a bucket indexes: `<home>/.basecraft/analysis-index/<bucket>`.
fn home_of(bucket: &Path) -> &Path {
    bucket.ancestors().nth(3).unwrap_or(bucket)
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
    basecraft_io::counters::record_read(path, bytes.len() as u64);
    let (record, writer) =
        basecraft_protocol::records::decode_written::<T>(&bytes).map_err(|e| {
            crate::Error::Damaged {
                path: path.into(),
                detail: e.to_string(),
            }
        })?;
    if version(&record) != 1 {
        return Err(crate::Error::Unsupported(
            basecraft_protocol::records::other_format(&writer, version(&record), 1),
        ));
    }
    Ok(record)
}

/// The record at `path` to add to, or `fresh` when there is none or it is
/// being rebuilt. A damaged record is an error unless it is being rebuilt.
fn existing<T: serde::de::DeserializeOwned>(
    path: &Path,
    version: fn(&T) -> u32,
    replace: bool,
    fresh: T,
) -> Result<T> {
    match read(path, version) {
        Ok(record) if !replace => Ok(record),
        Err(crate::Error::Io(error)) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(error.into())
        }
        Err(error @ (crate::Error::Damaged { .. } | crate::Error::Unsupported(_))) if !replace => {
            Err(error)
        }
        _ => Ok(fresh),
    }
}

fn write(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let bytes = basecraft_protocol::records::encode(value)?;
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

/// The file a bucket keeps for `store`, and the store's path as the file
/// records it, relative to the indexed folder. Named by that path, so a copied
/// store (which keeps its identity) gets its own file.
fn shard(bucket: &Path, store: &Path) -> Result<(PathBuf, String)> {
    let relative = super::relative(home_of(bucket), store)?.replace('\\', "/");
    let key = basecraft_io::hash_bytes(relative.as_bytes());
    let name = format!("{}.json", &key.trim_start_matches("b3:")[..32]);
    Ok((bucket.join(name), relative))
}

/// List `store` in the index for each of `sources`. The caller holds the
/// sources' reservations, so no save of them is choosing stores meanwhile.
pub(super) fn add(store: &Path, sources: &BTreeSet<PathBuf>, project: Option<&Path>) -> Result<()> {
    if is_private(store) {
        return Ok(());
    }
    let _store = lock_beside(&store.join("indexed-folders.lock"))?;
    record(store, sources, project, false).map(|_| ())
}

/// Rebuild `store`'s entries from the sources `collect` reads from its
/// ledgers: its file in each bucket and its list of buckets are replaced, and
/// its file in buckets no longer used is removed. The store's lock is held
/// from before the ledgers are read, so a writer adding meanwhile waits and
/// adds after. `collect` also says whether every ledger was readable; if not,
/// entries are only added. Copies a sync tool made are left.
pub(super) fn rebuild(
    store: &Path,
    project: Option<&Path>,
    collect: impl FnOnce() -> Result<(BTreeSet<PathBuf>, bool)>,
) -> Result<usize> {
    let store = std::path::absolute(store)?;
    let _store = lock_beside(&store.join("indexed-folders.lock"))?;
    let (sources, complete) = collect()?;
    if !complete {
        record(&store, &sources, project, false)?;
        return Ok(0);
    }
    let before = indexed_buckets(&store)?;
    let after = record(&store, &sources, project, true)?;
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

/// The caller holds the store's lock.
fn record(
    store: &Path,
    sources: &BTreeSet<PathBuf>,
    project: Option<&Path>,
    replace: bool,
) -> Result<BTreeSet<PathBuf>> {
    if is_private(store) {
        return Ok(BTreeSet::new());
    }
    let store = std::path::absolute(store)?;
    let mut homes = BTreeMap::<PathBuf, BTreeMap<PathBuf, BTreeSet<String>>>::new();
    for source in sources.iter().filter(|source| !is_private(source)) {
        if let Some((home, entry)) = home(source, project) {
            let bucket = bucket(&home, &entry);
            homes
                .entry(home)
                .or_default()
                .entry(bucket)
                .or_default()
                .insert(entry);
        }
    }
    let mut indexed = BTreeSet::new();
    for (home, buckets) in homes {
        indexed.extend(update(&home, &store, &buckets, replace)?);
    }
    list_buckets(&store, &indexed, replace)?;
    Ok(indexed)
}

/// Write this store's file in each of `buckets` of `home`'s index, and return
/// the buckets written. None when the folder itself refuses new files:
/// basecraft cannot save over a file there either.
fn update(
    home: &Path,
    store: &Path,
    buckets: &BTreeMap<PathBuf, BTreeSet<String>>,
    replace: bool,
) -> Result<Vec<PathBuf>> {
    let unwritable = |detail: String| {
        format!(
            "cannot record which analyses use files in {}: {detail}",
            home.display()
        )
    };
    if let Some(reason) = foreign_basecraft(home) {
        return Err(crate::Error::Unsupported(unwritable(reason.into())));
    }
    let result = (|| -> Result<()> {
        folder(&index_root(home))?;
        let _lock = lock_beside(&home.canonicalize()?.join(".basecraft/analysis-index.lock"))?;
        for (bucket, entries) in buckets {
            folder(bucket)?;
            let (path, relative) = shard(bucket, store)?;
            let fresh = AnalysisIndex {
                schema_version: 1,
                store: relative,
                files: BTreeSet::new(),
            };
            let mut index = existing(&path, |i: &AnalysisIndex| i.schema_version, replace, fresh)?;
            let before = index.clone();
            index.files.extend(entries.iter().cloned());
            if index != before || replace {
                write(&path, &index)?;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(buckets.keys().cloned().collect()),
        Err(crate::Error::Io(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            if tempfile::tempfile_in(home).is_err() {
                Ok(Vec::new())
            } else {
                Err(std::io::Error::new(error.kind(), unwritable(error.to_string())).into())
            }
        }
        Err(crate::Error::Io(error)) => {
            Err(std::io::Error::new(error.kind(), unwritable(error.to_string())).into())
        }
        Err(error) => Err(error),
    }
}

/// The index buckets that list `store`, from every copy of its list.
fn indexed_buckets(store: &Path) -> Result<BTreeSet<PathBuf>> {
    let home = store_home(store);
    let mut buckets = BTreeSet::new();
    for path in copies(store, "indexed-folders")? {
        let list = read(&path, |l: &IndexedFolders| l.schema_version)?;
        buckets.extend(list.folders.iter().map(|f| lexical(&home.join(f))));
    }
    Ok(buckets)
}

fn list_buckets(store: &Path, indexed: &BTreeSet<PathBuf>, replace: bool) -> Result<()> {
    let home = store_home(store);
    let path = store.join("indexed-folders.json");
    let fresh = IndexedFolders {
        schema_version: 1,
        folders: BTreeSet::new(),
    };
    let mut list = existing(&path, |l: &IndexedFolders| l.schema_version, replace, fresh)?;
    let before = list.clone();
    for bucket in indexed {
        list.folders
            .insert(super::relative(home, bucket)?.replace('\\', "/"));
    }
    if list != before || replace {
        write(&path, &list)?;
    }
    Ok(())
}

/// The index files in `bucket`, each with the store it names.
fn stores_in(bucket: &Path, inspection: &mut super::Inspection) -> Vec<(PathBuf, AnalysisIndex)> {
    let found = match copies(bucket, "") {
        Ok(found) => found,
        Err(error) => {
            inspection.issue(super::InspectionCode::RegistryUnavailable, bucket, error);
            return Vec::new();
        }
    };
    let home = home_of(bucket);
    let mut indexes = Vec::new();
    for path in found {
        match read(&path, |i: &AnalysisIndex| i.schema_version) {
            Ok(index) => indexes.push((lexical(&home.join(&index.store)), index)),
            Err(error) => {
                inspection.issue(super::InspectionCode::RegistryUnavailable, &path, error);
            }
        }
    }
    indexes
}

/// The stores listed for `source` in its index.
pub(super) fn listed(source: &Path, project: Option<&Path>) -> Result<Vec<PathBuf>> {
    let Some((home, entry)) = home(&lexical(source), project) else {
        return Ok(Vec::new());
    };
    let mut stores = Vec::new();
    for path in copies(&bucket(&home, &entry), "")? {
        let index = read(&path, |i: &AnalysisIndex| i.schema_version)?;
        if index.files.contains(&entry) {
            stores.push(lexical(&home.join(&index.store)));
        }
    }
    Ok(stores)
}

/// Stores that can refer to `target`. The index at the root of the session's
/// `project` (for a file inside it) and the index beside the file are read,
/// under either spelling of the path; a writer that did not know the project
/// indexed beside the file. A file with several hard links also matches the
/// entries in its folder's index for files that are the same file. Stores that
/// no longer exist are left out: their ledgers went with them. When the
/// project's root store lists this bucket of the project's index but the
/// bucket does not name the store, the index was lost: the store is inspected
/// and the check is incomplete, so the save asks.
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
    let mut places = BTreeSet::new();
    for spelling in [target.to_path_buf(), lexical(absolute)] {
        places.extend(home(&spelling, project));
        places.extend(home(&spelling, None));
    }
    let mut selected = BTreeSet::new();
    for (home, entry) in places {
        if let Some(reason) = foreign_basecraft(&home) {
            inspection.issue(
                super::InspectionCode::RegistryUnavailable,
                &home.join(".basecraft"),
                format!("{reason}; analyses of files here cannot be checked"),
            );
            continue;
        }
        let bucket = bucket(&home, &entry);
        let indexes = stores_in(&bucket, inspection);
        for (store, index) in &indexes {
            if index.files.contains(&entry) {
                selected.insert(store.clone());
            }
        }
        if linked {
            // Other names of the same file sit in other buckets of this index.
            for other in copies_of_dirs(&index_root(&home)) {
                for (store, index) in stores_in(&other, inspection) {
                    if index.files.iter().any(|f| same_file(&home.join(f))) {
                        selected.insert(store);
                    }
                }
            }
        }
        if project == Some(home.as_path()) {
            lost(&home, &bucket, &indexes, &mut selected, inspection);
        }
    }
    selected.retain(|store| store.try_exists().ok() != Some(false));
    selected.into_iter().collect()
}

fn copies_of_dirs(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

/// Projects whose stores were scanned for a lost index without a find. Only
/// a writer recreates the index, so one scan per process is enough.
static SCANNED: std::sync::Mutex<BTreeSet<PathBuf>> = std::sync::Mutex::new(BTreeSet::new());

/// A store that lists `bucket` while the bucket does not name it means the
/// index was lost. The project's root store is always checked; when the whole
/// index is missing, every store in the project is checked once.
fn lost(
    project: &Path,
    bucket: &Path,
    indexes: &[(PathBuf, AnalysisIndex)],
    selected: &mut BTreeSet<PathBuf>,
    inspection: &mut super::Inspection,
) {
    let mut stores = vec![project.join(".basecraft/analyses-v1")];
    let mut scanned = SCANNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !index_root(project).exists() && !scanned.contains(project) {
        // A project too large to scan is left to `reindex`.
        stores = super::stores_under(project).unwrap_or(stores);
    }
    let mut found = false;
    for store in stores {
        if indexes.iter().any(|(listed, _)| listed == &store) || !store.is_dir() {
            continue;
        }
        match indexed_buckets(&store) {
            Ok(buckets) if buckets.contains(&lexical(bucket)) => {
                found = true;
                selected.insert(store);
            }
            Ok(_) => {}
            Err(error) => inspection.issue(
                super::InspectionCode::RegistryUnavailable,
                &store.join("indexed-folders.json"),
                error,
            ),
        }
    }
    if found {
        inspection.issue(
            super::InspectionCode::RegistryUnavailable,
            bucket,
            format!(
                "the analysis index here is missing; `basecraft records reindex {}` rebuilds it",
                project.display()
            ),
        );
    } else if !index_root(project).exists() {
        scanned.insert(project.into());
    }
}

#[cfg(test)]
mod tests {
    /// A save replaces its target, so the file gets a new inode; the lock
    /// still holds by path. Likewise a file created after it was locked.
    #[test]
    fn a_replaced_or_created_file_stays_locked() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("gene.fasta");
        let created = root.path().join("new.fasta");
        std::fs::write(&target, b">a\nACGT\n").unwrap();
        let locks = super::super::lock().unwrap();
        locks.source(&target).unwrap();
        locks.source(&created).unwrap();
        basecraft_io::write_atomic(&target, |w| std::io::Write::write_all(w, b">a\nTCGT\n"))
            .unwrap();
        std::fs::write(&created, b">b\nACGT\n").unwrap();
        std::thread::spawn(move || {
            for path in [target, created] {
                let other = super::super::lock().unwrap();
                assert!(
                    matches!(other.source(&path), Err(crate::Error::Busy(_))),
                    "{} was not locked",
                    path.display()
                );
            }
        })
        .join()
        .unwrap();
    }

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
        super::add(store, &[introduced.canonicalize().unwrap()].into(), None).unwrap();
        std::thread::spawn(move || {
            let mut session = crate::Session::new(other.parent().unwrap());
            let doc = session.open(&other).unwrap();
            session
                .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
                .unwrap();
            session.save(doc, None).unwrap();
            assert_eq!(std::fs::read(&other).unwrap(), b">a\nTCGT\n");
            // Both the old index and the newly introduced source stay protected.
            for path in [source, introduced] {
                let doc = session.open(&path).unwrap();
                session
                    .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
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
        let _first = super::super::lock_save(&target, &[], None).unwrap();
        std::thread::spawn(move || {
            let mut session = crate::Session::new(other.parent().unwrap());
            let id = session.open(&other).unwrap();
            session
                .apply(id, basecraft_protocol::Op::seq_replace(0, 1, "T"))
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
    fn results_with_history_in_one_store_save_concurrently() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        // Two analysis results whose history bundles are runs of one store.
        for name in ["a.fasta", "b.fasta"] {
            let output = root.path().join(name);
            std::fs::write(&output, b">a\nACGT\n").unwrap();
            let run =
                crate::records::analysis_store::ArtifactRun::create(None, &output, &name[..1])
                    .unwrap();
            run.bind(None, &output, &basecraft_io::hash_file(&output).unwrap())
                .unwrap();
        }
        let savers: Vec<_> = ["a.fasta", "b.fasta"]
            .into_iter()
            .map(|name| {
                let root = root.path().to_path_buf();
                std::thread::spawn(move || {
                    let mut session = crate::Session::recoverable(&root).unwrap();
                    let doc = session.open(&root.join(name)).unwrap();
                    session
                        .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
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
