//! Analysis input index. Writers record, per input path and per file identity,
//! the stores that can refer to it; a save reads only the shards for its target.
use super::{
    ArtifactRun, BTreeMap, Path, PathBuf, Result, lexical, read_input, registry, registry_lock,
    stamp, validate_input,
};
use basecraft_protocol::records::{
    AnalysisInputIndex, IndexChange, IndexFile, IndexedAnalysisStore,
};
use std::collections::BTreeSet;

fn location() -> Result<PathBuf> {
    Ok(registry()?.with_file_name("analysis-input-index.json"))
}
fn shard(kind: &str, path: &Path) -> Result<PathBuf> {
    let key = basecraft_io::hash_bytes(path.as_os_str().as_encoded_bytes());
    Ok(registry()?
        .with_file_name("analysis-inputs")
        .join(kind)
        .join(key.trim_start_matches("b3:")))
}
fn read<T: serde::de::DeserializeOwned + serde::Serialize>(path: &Path) -> Result<T> {
    let bytes = crate::records::provenance::read_record(path)?;
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::record_read(path, bytes.len() as u64);
    let record: IndexFile<T> =
        serde_json::from_slice(&bytes).map_err(|e| crate::Error::Damaged {
            path: path.into(),
            detail: format!("{e}; revision {}", basecraft_io::hash_bytes(&bytes)),
        })?;
    if basecraft_io::hash_bytes(&serde_json::to_vec(&record.value)?) != record.checksum {
        return Err(crate::Error::Damaged {
            path: path.into(),
            detail: format!(
                "analysis index checksum differs; revision {}",
                basecraft_io::hash_bytes(&bytes)
            ),
        });
    }
    Ok(record.value)
}
fn write(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let bytes = serde_json::to_vec(&IndexFile {
        checksum: basecraft_io::hash_bytes(&serde_json::to_vec(value)?),
        value,
    })?;
    basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
    // A durable marker must precede the ledger change; a durable index must
    // precede clearing that marker. Include newly created shard directories.
    let state = registry()?.parent().unwrap().to_path_buf();
    for directory in path
        .parent()
        .unwrap()
        .ancestors()
        .take_while(|p| p.starts_with(&state))
    {
        basecraft_platform::files::sync_directory(directory)?;
    }
    Ok(())
}
fn load() -> Result<AnalysisInputIndex> {
    let path = location()?;
    if !path.try_exists()? {
        return Ok(AnalysisInputIndex {
            schema_version: 2,
            stores: BTreeMap::new(),
            sources: Vec::new(),
        });
    }
    let index: AnalysisInputIndex = read(&path)?;
    if index.schema_version != 2 {
        return Err(crate::Error::Unsupported(
            "analysis input index version".into(),
        ));
    }
    Ok(index)
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

fn snapshot(store: &Path) -> Result<IndexedAnalysisStore> {
    ArtifactRun::validate_store(store)?;
    let identity = std::fs::read_to_string(store.join("identity"))?;
    let mut index = IndexedAnalysisStore {
        identity,
        sources: Vec::new(),
        dependencies: Vec::new(),
        extra: BTreeMap::new(),
    };

    let mut budget = 64 << 20;
    for (count, entry) in std::fs::read_dir(store.join("runs"))?.enumerate() {
        if count >= 4096 {
            return Err(crate::Error::Limit {
                what: "analysis store runs".into(),
                limit: 4096,
            });
        }
        let entry = entry?;
        let run = entry.path();
        // Ordinary stray files cannot contain an input ledger.
        let kind = entry.file_type()?;
        if kind.is_file() {
            continue;
        }
        if !kind.is_dir() {
            return Err(crate::Error::Damaged {
                path: run,
                detail: "unsafe entry in analysis store".into(),
            });
        }
        let path = run.join("input.json");
        if !path.try_exists()? {
            continue;
        }
        let bytes = crate::records::provenance::read_record_with_budget(&path, &mut budget)?;
        let input = validate_input(&run, &bytes)?;
        if input.state == basecraft_protocol::input_records::EvidenceState::Referenced
            && input.proposal_target != Some(true)
            && let Some(source) = &input.source
        {
            index.sources.extend(spellings(&run.join(&source.path)));
        }
        if let Some(owner) = &input.analysis_run {
            index.dependencies.extend(spellings(&run.join(owner)));
        }
        if let Some(value) = input.extra.get("origin_run") {
            let path = value.as_str().ok_or_else(|| crate::Error::Damaged {
                path: run.clone(),
                detail: "invalid origin_run".into(),
            })?;
            index.dependencies.extend(spellings(&run.join(path)));
        }
        {
            for parent in &input.upstream {
                let path = &parent.bundle;
                // Resolve current ownership while publishing the index.
                let mut path = lexical(&run.join(path));
                let mut seen = BTreeSet::new();
                loop {
                    index.dependencies.extend(spellings(&path));
                    if !seen.insert(path.clone()) || seen.len() > 32 {
                        return Err(crate::Error::Limit {
                            what: "input ancestry".into(),
                            limit: 32,
                        });
                    }
                    let ancestor = read_input(&path)?;
                    let Some(origin) = ancestor.extra.get("origin_run") else {
                        break;
                    };
                    let origin = origin.as_str().ok_or_else(|| crate::Error::Damaged {
                        path: path.clone(),
                        detail: "invalid origin location".into(),
                    })?;
                    path = lexical(&path.join(origin));
                }
            }
        }
    }
    index.sources.sort();
    index.sources.dedup();
    index.dependencies.sort();
    index.dependencies.dedup();
    Ok(index)
}

/// Input writes publish a durable dirty marker before touching their ledger.
/// A failed update stays incomplete, including lookups for newly added sources.
/// Basecraft's own state and cache folders hold recovery staging, never an
/// analysis store a user's file can depend on. They are never indexed.
pub(super) fn is_private(store: &Path) -> bool {
    let Ok(paths) = basecraft_platform::paths::AppPaths::discover() else {
        return false;
    };
    let store = store.canonicalize().unwrap_or_else(|_| lexical(store));
    [paths.state, paths.cache]
        .iter()
        .any(|root| store.starts_with(root.canonicalize().unwrap_or_else(|_| lexical(root))))
}

pub(super) fn changing(path: &Path, sources: Option<BTreeSet<PathBuf>>) -> Result<Option<PathBuf>> {
    let Some(run) = path.parent() else {
        return Ok(None);
    };
    let Some(runs) = run
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "runs"))
    else {
        return Ok(None);
    };
    let Some(store) = runs
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "analyses-v1"))
    else {
        return Ok(None);
    };
    if is_private(store) {
        return Ok(None);
    }
    let marker = shard("dirty", store)?;
    let sources = match read::<IndexChange>(&marker) {
        Ok(previous) => previous.sources.zip(sources).map(|(mut old, new)| {
            old.extend(new);
            old
        }),
        Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => sources,
        Err(_) => None,
    };
    write(
        &marker,
        &IndexChange {
            store: store.into(),
            sources,
        },
    )?;
    Ok(Some(store.into()))
}

pub(super) fn register(store: &Path, guard: Option<std::fs::File>) -> Result<()> {
    if is_private(store) {
        return Ok(());
    }
    let entry = match snapshot(store) {
        Ok(entry) => entry,
        Err(error @ crate::Error::Busy(_)) => return Err(error),
        Err(_) => {
            let _guard = guard.map_or_else(registry_lock, Ok)?;
            let marker = shard("dirty", store)?;
            // A store that is gone holds no ledger anything can depend on.
            if !store.try_exists()? {
                match std::fs::remove_file(&marker) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(error.into());
                    }
                    _ => return Ok(()),
                }
            }
            if !marker.try_exists()? {
                write(
                    &marker,
                    &IndexChange {
                        store: store.into(),
                        sources: None,
                    },
                )?;
            }
            return Ok(());
        }
    };
    let _guard = guard.map_or_else(registry_lock, Ok)?;
    publish(store, &entry)
}

fn publish(store: &Path, entry: &IndexedAnalysisStore) -> Result<()> {
    let mut index = load()?;
    // Registration of the current location retires its old index location.
    let retired: BTreeSet<_> = index
        .stores
        .iter()
        .filter(|(path, identity)| {
            path.as_path() != store
                && *identity == &entry.identity
                && path.try_exists().ok() == Some(false)
        })
        .map(|(path, _)| path.clone())
        .collect();
    index.stores.retain(|path, _| !retired.contains(path));
    if !retired.is_empty() {
        let mut registry_record = super::store_registry()?;
        registry_record
            .stores
            .retain(|path| !retired.contains(path));
        if !registry_record.stores.contains(&store.to_path_buf()) {
            registry_record.stores.push(store.into());
        }
        super::write_json(&registry()?, &registry_record)?;
    }
    write(&shard("stores", store)?, entry)?;
    index.stores.insert(store.into(), entry.identity.clone());
    let mut entries = BTreeMap::new();
    for path in index.stores.keys() {
        entries.insert(
            path.clone(),
            read::<IndexedAnalysisStore>(&shard("stores", path)?)?,
        );
    }
    // Compute connected ownership once when records change, never on each save.
    let mut sources: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    for (path, entry) in &entries {
        for source in &entry.sources {
            sources
                .entry(source.clone())
                .or_default()
                .insert(path.clone());
        }
    }
    for selected in sources.values_mut() {
        loop {
            let before = selected.len();
            for (path, entry) in &entries {
                for dependency in &entry.dependencies {
                    for other in entries.keys().filter(|other| dependency.starts_with(other)) {
                        if selected.contains(path) || selected.contains(other) {
                            selected.insert(path.clone());
                            selected.insert(other.clone());
                        }
                    }
                }
            }
            if before == selected.len() {
                break;
            }
        }
    }
    // Each source is also found by its canonical spelling and file identity.
    let mut keys = sources.clone();
    let mut files: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    for (source, stores) in &sources {
        if let Ok(canonical) = source.canonicalize() {
            keys.entry(canonical)
                .or_default()
                .extend(stores.iter().cloned());
        }
        if let Some(key) = file_key(source) {
            files.entry(key).or_default().insert(source.clone());
        }
    }
    for (kind, entries) in [("sources", &keys), ("files", &files)] {
        for (key, value) in entries {
            let path = shard(kind, key)?;
            if read::<BTreeSet<PathBuf>>(&path).ok().as_ref() != Some(value) {
                write(&path, value)?;
            }
        }
    }
    for stale in index.sources.iter().filter(|p| !keys.contains_key(*p)) {
        match std::fs::remove_file(shard("sources", stale)?) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
    index.sources = keys.into_keys().collect();
    write(&location()?, &index)?;
    let dirty = shard("dirty", store)?;
    if dirty.try_exists()? {
        std::fs::remove_file(&dirty)?;
        basecraft_platform::files::sync_directory(dirty.parent().unwrap())?;
    }
    Ok(())
}

fn file_key(path: &Path) -> Option<PathBuf> {
    let stamp = stamp(path).ok()?;
    Some(PathBuf::from(format!("{}:{}", stamp.device?, stamp.inode?)))
}

/// A missing shard means nothing is recorded for that key. A damaged one makes
/// the check incomplete without hiding what other shards record.
fn shard_or_issue<T: serde::de::DeserializeOwned + serde::Serialize>(
    path: &Path,
    inspection: &mut super::Inspection,
) -> Option<T> {
    match read(path) {
        Ok(value) => Some(value),
        Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            inspection.issue(super::InspectionCode::RegistryUnavailable, path, error);
            None
        }
    }
}

/// Stores that can refer to `target`: the shards for its path spellings and file
/// identity, pending ledger changes, and ancestor stores the index does not cover.
/// Only a target with several hard links compares against every indexed source.
pub(super) fn select(
    target: &Path,
    absolute: &Path,
    nearby: &[PathBuf],
    inspection: &mut super::Inspection,
) -> Result<Vec<PathBuf>> {
    let identity = file_key(target);
    let same_file = |path: &Path| identity.is_some() && file_key(path) == identity;
    let mut sources = BTreeSet::from([target.to_path_buf(), lexical(absolute)]);
    if let Some(key) = &identity {
        // A moved or hard-linked source keeps the identity recorded at registration.
        let recorded: Option<BTreeSet<PathBuf>> = shard_or_issue(&shard("files", key)?, inspection);
        sources.extend(
            recorded
                .into_iter()
                .flatten()
                .filter(|path| path.try_exists().ok() == Some(false) || same_file(path)),
        );
        if basecraft_platform::files::link_count(target)? > 1 {
            match load() {
                Ok(index) => sources.extend(index.sources.into_iter().filter(|p| same_file(p))),
                Err(error) => inspection.issue(
                    super::InspectionCode::RegistryUnavailable,
                    &location()?,
                    error,
                ),
            }
        }
    }
    let mut selected = BTreeSet::new();
    for source in &sources {
        let stores: Option<BTreeSet<PathBuf>> =
            shard_or_issue(&shard("sources", source)?, inspection);
        selected.extend(stores.into_iter().flatten());
    }
    let dirty = registry()?.with_file_name("analysis-inputs/dirty");
    let dirty_result = (|| -> Result<()> {
        if dirty.try_exists()? {
            for entry in std::fs::read_dir(&dirty)? {
                let path = entry?.path();
                let change: IndexChange = read(&path)?;
                // A pending change in a store that no longer exists cannot
                // concern this target: its ledgers went with it.
                if change.store.try_exists().ok() == Some(false) {
                    continue;
                }
                if change.sources.as_ref().is_some_and(|pending| {
                    !pending.iter().any(|p| sources.contains(p) || same_file(p))
                }) {
                    continue;
                }
                inspection.record(&path)?;
                inspection.issue(
                    super::InspectionCode::RegistryUnavailable,
                    &path,
                    "analysis index update is incomplete",
                );
                selected.insert(change.store);
            }
        }
        Ok(())
    })();
    if let Err(error) = dirty_result {
        inspection.issue(super::InspectionCode::RegistryUnavailable, &dirty, error);
    }
    let mut moved = Vec::new();
    for store in nearby {
        let Ok(identity) = std::fs::read_to_string(store.join("identity")) else {
            selected.insert(store.clone());
            continue;
        };
        let indexed: Option<IndexedAnalysisStore> =
            shard_or_issue(&shard("stores", store)?, inspection);
        if indexed.is_none_or(|entry| entry.identity != identity) {
            selected.insert(store.clone());
            moved.push(identity);
        }
    }
    // A store found in a new ancestor location replaces its vanished old one.
    selected.retain(|store| {
        moved.is_empty()
            || store.try_exists().ok() != Some(false)
            || read::<IndexedAnalysisStore>(&shard("stores", store).unwrap_or_default())
                .is_ok_and(|entry| !moved.contains(&entry.identity))
    });
    Ok(selected.into_iter().collect())
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
        super::changing(
            &run.directory().join("input.json"),
            Some([introduced.clone()].into()),
        )
        .unwrap();
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
        let _first = super::super::lock_save(&target, false, &[]).unwrap();
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
    fn saves_share_a_store_and_wait_for_a_writer() {
        crate::isolated_profile::isolate!();
        let root = tempfile::tempdir().unwrap();
        // An ancestor store the index does not cover is read by every save here.
        let run = crate::records::analysis_store::ArtifactRun::create(
            &root.path().join("result.txt"),
            "x",
        )
        .unwrap();
        let store = run
            .directory()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        for name in ["a/a.fasta", "b/b.fasta"] {
            let file = root.path().join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b">a\nACGT\n").unwrap();
        }
        let (held, ready) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let locks = super::super::lock().unwrap();
            locks.reserve([store], true).unwrap();
            held.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
        });
        ready.recv().unwrap();
        let savers: Vec<_> = ["a/a.fasta", "b/b.fasta"]
            .into_iter()
            .map(|name| {
                let file = root.path().join(name);
                std::thread::spawn(move || {
                    let mut session = crate::Session::new(file.parent().unwrap());
                    let doc = session.open(&file).unwrap();
                    session
                        .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
                        .unwrap();
                    session.save(doc, None).unwrap();
                })
            })
            .collect();
        for saver in savers {
            saver.join().unwrap();
        }
        writer.join().unwrap();
        for name in ["a/a.fasta", "b/b.fasta"] {
            assert_eq!(
                std::fs::read(root.path().join(name)).unwrap(),
                b">a\nTCGT\n"
            );
        }
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
