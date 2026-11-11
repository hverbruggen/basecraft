//! Current-format source index. Saves stat source paths for external aliases,
//! then open only the matching source shards. Writers replace the index explicitly.
use super::{
    ArtifactRun, BTreeMap, PROPOSAL_TARGET, Path, PathBuf, Result, lexical, read_input, registry,
    registry_lock, stamp, validate_input,
};
use basecraft_protocol::records::{AnalysisInputIndex, IndexFile, IndexedAnalysisStore};
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
    let bytes = crate::provenance::read_record(path)?;
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
                // Resolve current ownership while publishing the index.
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
pub(super) fn changing(path: &Path) -> Result<Option<PathBuf>> {
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
    write(&shard("dirty", store)?, &store)?;
    Ok(Some(store.into()))
}

pub(super) fn register(store: &Path) -> Result<()> {
    let entry = match snapshot(store) {
        Ok(entry) => entry,
        Err(error @ crate::Error::Busy(_)) => return Err(error),
        Err(_) => {
            let _guard = registry_lock()?;
            write(&shard("dirty", store)?, &store)?;
            return Ok(());
        }
    };
    let _guard = registry_lock()?;
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
    for (source, stores) in &sources {
        let path = shard("sources", source)?;
        if read::<BTreeSet<PathBuf>>(&path).ok().as_ref() != Some(stores) {
            write(&path, stores)?;
        }
    }
    index.sources = sources.into_keys().collect();
    write(&location()?, &index)?;
    let dirty = shard("dirty", store)?;
    if dirty.try_exists()? {
        std::fs::remove_file(&dirty)?;
        basecraft_platform::files::sync_directory(dirty.parent().unwrap())?;
    }
    Ok(())
}

pub(super) fn select(
    target: &Path,
    stores: &[PathBuf],
    inspection: &mut super::Inspection,
) -> Result<Vec<PathBuf>> {
    let index = load()?;
    let mut selected = BTreeSet::new();
    let mut relocated = BTreeSet::new();
    let dirty = registry()?.with_file_name("analysis-inputs/dirty");
    let dirty_result = (|| -> Result<()> {
        if dirty.try_exists()? {
            for entry in std::fs::read_dir(&dirty)? {
                let path = entry?.path();
                inspection.record(&path)?;
                let store: PathBuf = read(&path)?;
                inspection.issue(
                    super::InspectionCode::RegistryUnavailable,
                    &path,
                    "analysis index update is incomplete",
                );
                selected.insert(store);
            }
        }
        Ok(())
    })();
    if let Err(error) = dirty_result {
        inspection.issue(super::InspectionCode::RegistryUnavailable, &dirty, error);
    }
    for store in stores {
        if index.stores.contains_key(store) {
            continue;
        }
        selected.insert(store.clone());
        if let Ok(identity) = std::fs::read_to_string(store.join("identity")) {
            // Moved current-format stores retain their immutable identity.
            for (old, known) in &index.stores {
                if *known == identity && old.try_exists().ok() == Some(false) {
                    relocated.insert(old.clone());
                }
            }
        }
    }
    selected.retain(|path| !relocated.contains(path));
    let target_stamp = stamp(target).ok();
    for source in &index.sources {
        let matches = lexical(source) == lexical(target)
            || source.canonicalize().ok().as_deref() == Some(target)
            || match (stamp(source), &target_stamp) {
                (Ok(a), Some(b)) => {
                    a.device.is_some() && a.device == b.device && a.inode == b.inode
                }
                (Ok(_), None) => false,
                (Err(_), _) => true,
            };
        if matches {
            let path = shard("sources", source)?;
            match read::<Vec<PathBuf>>(&path) {
                Ok(stores) => {
                    selected.extend(stores.into_iter().filter(|p| !relocated.contains(p)));
                }
                Err(error) => {
                    inspection.issue(super::InspectionCode::RegistryUnavailable, &path, error);
                }
            }
        }
    }
    Ok(selected.into_iter().collect())
}

#[cfg(test)]
mod tests {

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
        let _first = super::super::lock_save(&target).unwrap();
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
                );
                assert!(matches!(result, Err(crate::Error::Busy(_))));
            }
        })
        .join()
        .unwrap();
    }
}
