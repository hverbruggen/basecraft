//! Private lookup cache. Live source stats retain external-alias detection;
//! ledger and directory stats detect changes by writers that know no index.
use super::{
    ArtifactRun, BTreeMap, PROPOSAL_TARGET, Path, PathBuf, Result, lexical, read_input, registry,
    registry_lock, stamp, validate_input, write_json,
};
use basecraft_protocol::records::{AnalysisInputIndex, IndexFileStamp, IndexedAnalysisStore};
use std::collections::BTreeSet;

fn location() -> Result<PathBuf> {
    Ok(registry()?.with_file_name("analysis-input-index.json"))
}

fn load() -> AnalysisInputIndex {
    location()
        .and_then(|path| {
            let bytes = crate::provenance::read_record(&path)?;
            let index: AnalysisInputIndex = serde_json::from_slice(&bytes)?;
            if index.schema_version != 1 {
                return Err(crate::Error::Unsupported(
                    "unknown analysis input index".into(),
                ));
            }
            Ok(index)
        })
        .unwrap_or_else(|_| AnalysisInputIndex {
            schema_version: 1,
            stores: BTreeMap::new(),
            extra: BTreeMap::new(),
        })
}

fn metadata(path: &Path) -> Result<Option<IndexFileStamp>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if meta.file_type().is_symlink() || !(meta.is_file() || meta.is_dir()) {
        return Err(crate::Error::Damaged {
            path: path.into(),
            detail: "unsafe index metadata entry".into(),
        });
    }
    let (identity, changed_seconds, changed_nanos) = metadata_identity(&meta)?;
    let modified = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| crate::Error::Damaged {
            path: path.into(),
            detail: e.to_string(),
        })?;
    Ok(Some(IndexFileStamp {
        identity,
        size: meta.len(),
        modified_seconds: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
        changed_seconds,
        changed_nanos,
    }))
}

#[cfg(unix)]
fn metadata_identity(meta: &std::fs::Metadata) -> Result<(String, i64, i64)> {
    use std::os::unix::fs::MetadataExt;
    let identity = basecraft_io::hash_bytes(&serde_json::to_vec(&(meta.dev(), meta.ino()))?);
    Ok((identity, meta.ctime(), meta.ctime_nsec()))
}
#[cfg(not(unix))]
fn metadata_identity(_: &std::fs::Metadata) -> Result<(String, i64, i64)> {
    Err(crate::Error::Unsupported(
        "analysis index requires reliable change metadata; using full inspection".into(),
    ))
}

fn remember(index: &mut IndexedAnalysisStore, path: &Path) -> Result<()> {
    index.files.insert(path.into(), metadata(path)?);
    Ok(())
}

fn snapshot(store: &Path) -> Result<IndexedAnalysisStore> {
    ArtifactRun::validate_store(store)?;
    let identity = metadata(store)?
        .ok_or_else(|| crate::Error::NotFound { path: store.into() })?
        .identity;
    let mut index = IndexedAnalysisStore {
        identity,
        checksum: String::new(),
        files: BTreeMap::new(),
        sources: Vec::new(),
        dependencies: Vec::new(),
        extra: BTreeMap::new(),
    };
    remember(&mut index, store)?;
    remember(&mut index, &store.join("store.json"))?;
    remember(&mut index, &store.join("runs"))?;
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
        remember(&mut index, &run)?;
        let path = run.join("input.json");
        remember(&mut index, &path)?;
        if index.files[&path].is_none() {
            continue;
        }
        let bytes = crate::provenance::read_record_with_budget(&path, &mut budget)?;
        let input = validate_input(&run, &bytes)?;
        if input.state == "referenced"
            && input.extra.get(PROPOSAL_TARGET) != Some(&serde_json::Value::Bool(true))
            && let Some(source) = &input.source
        {
            index.sources.push(lexical(&run.join(&source.path)));
        }
        for field in ["analysis_run", "origin_run"] {
            if let Some(value) = input.extra.get(field) {
                let path = value.as_str().ok_or_else(|| crate::Error::Damaged {
                    path: run.clone(),
                    detail: format!("invalid {field}"),
                })?;
                index.dependencies.push(lexical(&run.join(path)));
            }
        }
        if let Some(parents) = input.extra.get("upstream") {
            let parents = parents.as_array().ok_or_else(|| crate::Error::Damaged {
                path: run.clone(),
                detail: "invalid upstream inventory".into(),
            })?;
            for parent in parents {
                let path = parent["bundle"]
                    .as_str()
                    .ok_or_else(|| crate::Error::Damaged {
                        path: run.clone(),
                        detail: "ancestor location is missing".into(),
                    })?;
                // Match the scanner's delegation walk; retain each intermediate
                // input's metadata so changing an external companion invalidates us.
                let mut path = lexical(&run.join(path));
                let mut seen = BTreeSet::new();
                loop {
                    index.dependencies.push(path.clone());
                    if !seen.insert(path.clone()) || seen.len() > 32 {
                        return Err(crate::Error::Limit {
                            what: "input ancestry".into(),
                            limit: 32,
                        });
                    }
                    remember(&mut index, &path.join("input.json"))?;
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
    index.checksum = checksum(&index)?;
    Ok(index)
}

fn checksum(index: &IndexedAnalysisStore) -> Result<String> {
    Ok(basecraft_io::hash_bytes(&serde_json::to_vec(&(
        &index.identity,
        &index.files,
        &index.sources,
        &index.dependencies,
    ))?))
}

fn current(index: &IndexedAnalysisStore) -> bool {
    if checksum(index).ok().as_ref() != Some(&index.checksum) {
        return false;
    }
    index
        .files
        .iter()
        .all(|(path, expected)| metadata(path).is_ok_and(|actual| &actual == expected))
}

/// Registration already holds the evidence mutation lock. Cache failures never
/// undo a durable registration: the next lookup performs the conservative scan.
pub(super) fn register(store: &Path) {
    let entry = snapshot(store);
    let Ok(_guard) = registry_lock() else {
        return;
    };
    let mut index = load();
    if let Ok(mut entry) = entry {
        if let Some(old) = index.stores.remove(store) {
            entry.extra = old.extra;
        }
        index.stores.insert(store.into(), entry);
    } else {
        index.stores.remove(store);
    }
    if let Ok(path) = location() {
        let _ = write_json(&path, &index);
    }
}

pub(super) fn select(target: &Path, stores: &[PathBuf], persist: bool) -> Vec<PathBuf> {
    let mut index = load();
    let mut unknown = BTreeSet::new();
    let mut changed = false;
    for store in stores {
        if index.stores.get(store).is_some_and(current) {
            continue;
        }
        match snapshot(store) {
            Ok(mut entry) => {
                if let Some(old) = index.stores.remove(store) {
                    entry.extra = old.extra;
                }
                index.stores.insert(store.clone(), entry);
                changed = true;
            }
            Err(_) => {
                unknown.insert(store.clone());
            }
        }
    }
    // A recorded identity can prove that an old missing location moved to an
    // ancestor store. An unexplained missing/inaccessible store stays unknown.
    let mut moved = BTreeSet::new();
    unknown.retain(|store| {
        if !matches!(std::fs::symlink_metadata(store), Err(e) if e.kind() == std::io::ErrorKind::NotFound) { return true; }
        let Some(old) = index.stores.get(store) else { return true; };
        let relocated = stores.iter().any(|other| other != store && !unknown_location(other) && index.stores.get(other).is_some_and(|entry| same_store(store, old, other, entry)));
        if relocated { moved.insert(store.clone()); }
        !relocated
    });
    let mut selected = unknown;
    let target_stamp = stamp(target).ok();
    let mut sources = BTreeMap::new();
    for store in stores {
        if moved.contains(store) {
            continue;
        }
        let Some(entry) = index.stores.get(store) else {
            selected.insert(store.clone());
            continue;
        };
        for source in &entry.sources {
            let matches = *sources.entry(source.clone()).or_insert_with(|| {
                if lexical(source) == lexical(target)
                    || source.canonicalize().ok().as_deref() == Some(target)
                {
                    return true;
                }
                match (stamp(source), &target_stamp) {
                    (Ok(a), Some(b)) => {
                        a.device.is_some() && a.device == b.device && a.inode == b.inode
                    }
                    (Ok(_), None) => false,
                    (Err(_), _) => true, // unknown alias coverage is never unrelated
                }
            });
            if matches {
                selected.insert(store.clone());
            }
        }
    }
    // Include reverse descendants AND all backing owners. Undirected store
    // closure is conservative and keeps multi-input ownership decisions intact.
    loop {
        let before = selected.len();
        for store in stores {
            if let Some(entry) = index.stores.get(store) {
                for dependency in &entry.dependencies {
                    for other in stores.iter().filter(|other| dependency.starts_with(other)) {
                        if selected.contains(store) || selected.contains(other) {
                            selected.insert(store.clone());
                            selected.insert(other.clone());
                        }
                    }
                }
            }
        }
        if before == selected.len() {
            break;
        }
    }
    if persist
        && changed
        && let (Ok(_guard), Ok(path)) = (registry_lock(), location())
    {
        let _ = write_json(&path, &index);
    }
    stores
        .iter()
        .filter(|store| selected.contains(*store))
        .cloned()
        .collect()
}

// Directory identifiers can be recycled after deletion. A matching, uniquely
// named retained run must survive too; directory identity alone proves no move.
fn same_store(
    old_path: &Path,
    old: &IndexedAnalysisStore,
    new_path: &Path,
    new: &IndexedAnalysisStore,
) -> bool {
    checksum(old).ok().as_ref() == Some(&old.checksum)
        && checksum(new).ok().as_ref() == Some(&new.checksum)
        && old.identity == new.identity
        && old.files.iter().any(|(path, before)| {
            let Ok(relative) = path.strip_prefix(old_path.join("runs")) else {
                return false;
            };
            relative.components().count() == 2
                && relative
                    .file_name()
                    .is_some_and(|name| name == "input.json")
                && before.as_ref().is_some_and(|before| {
                    new.files
                        .get(&new_path.join("runs").join(relative))
                        .and_then(Option::as_ref)
                        .is_some_and(|after| before == after)
                })
        })
}

fn unknown_location(path: &Path) -> bool {
    !path.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_evidence::{lock, lock_save};
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn unrelated_saves_share_gate_but_registration_waits() {
        let Some(root) = crate::isolated_profile::enter(
            "input_evidence::index::tests::unrelated_saves_share_gate_but_registration_waits",
        ) else {
            return;
        };
        let a = root.join("a.fasta");
        let b = root.join("b.fasta");
        std::fs::write(&a, b">a\nACGT\n").unwrap();
        std::fs::write(&b, b">b\nACGT\n").unwrap();
        let first = lock_save(&a, false).unwrap();
        let (sent, received) = mpsc::channel();
        let save = std::thread::spawn(move || {
            let mut session = crate::Session::new(b.parent().unwrap());
            let id = session.open(&b).unwrap();
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
            sent.send(()).unwrap();
        });
        received
            .recv_timeout(Duration::from_secs(3))
            .expect("unrelated save must not wait for the other save");
        save.join().unwrap();
        let (sent, received) = mpsc::channel();
        let registrar = std::thread::spawn(move || {
            let _lock = lock().unwrap();
            sent.send(()).unwrap();
        });
        assert!(received.recv_timeout(Duration::from_millis(100)).is_err());
        drop(first);
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        registrar.join().unwrap();
    }
}
