//! Analysis index beside the files analyses used. Each folder holding such a
//! file keeps `.basecraft/analysis-index.json`: for each file name, the stores
//! whose runs depend on it. A writer adds its store before it changes a ledger;
//! entries are never removed, so the index may list more than is current, never
//! less. A save reads only the index beside its own file.
use super::{Path, PathBuf, Result, lexical, stamp};
use basecraft_protocol::records::AnalysisIndex;
use fs4::fs_std::FileExt;
use std::collections::BTreeSet;

pub(crate) const FILE: &str = "analysis-index.json";

fn location(folder: &Path) -> PathBuf {
    folder.join(".basecraft").join(FILE)
}

fn read(path: &Path) -> Result<Option<AnalysisIndex>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        // No index, or a `.basecraft` file that belongs to another program.
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::record_read(path, bytes.len() as u64);
    let index: AnalysisIndex =
        serde_json::from_slice(&bytes).map_err(|e| crate::Error::Damaged {
            path: path.into(),
            detail: e.to_string(),
        })?;
    if index.schema_version != 1 {
        return Err(crate::Error::Unsupported("analysis index version".into()));
    }
    Ok(Some(index))
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

/// List `store` in the index beside each of `sources`. The caller holds the
/// sources' reservations, so no save of them is choosing stores meanwhile.
pub(super) fn add(store: &Path, sources: &BTreeSet<PathBuf>) -> Result<()> {
    if is_private(store) {
        return Ok(());
    }
    let store = std::path::absolute(store)?;
    let mut folders = std::collections::BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for source in sources.iter().filter(|source| !is_private(source)) {
        if let (Some(folder), Some(name)) = (source.parent(), source.file_name()) {
            let name = name
                .to_str()
                .ok_or_else(|| crate::error::invalid("input file name is not UTF-8"))?;
            folders
                .entry(folder.into())
                .or_default()
                .insert(name.into());
        }
    }
    for (folder, names) in folders {
        let relative = super::relative(&folder, &store)?.replace('\\', "/");
        match update(&folder, &names, &relative) {
            // Basecraft cannot save over a file in a folder it cannot write.
            Err(crate::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
                ) => {}
            result => result?,
        }
    }
    Ok(())
}

fn update(folder: &Path, names: &BTreeSet<String>, store: &str) -> Result<()> {
    let hidden = folder.join(".basecraft");
    // A `.basecraft` that is not a folder belongs to another program; basecraft
    // keeps no records there, as with its analysis stores.
    if std::fs::symlink_metadata(&hidden).is_ok_and(|m| !m.is_dir()) {
        return Ok(());
    }
    let path = location(folder);
    // Held only for this read and write, never while waiting for anything else.
    let lock = super::open_lock(&super::lock_path(&path)?)?;
    lock.lock_exclusive()
        .map_err(|e| crate::Error::Busy(e.to_string()))?;
    let mut index = read(&path)?.unwrap_or(AnalysisIndex {
        schema_version: 1,
        files: std::collections::BTreeMap::new(),
    });
    let mut changed = false;
    for name in names {
        changed |= index
            .files
            .entry(name.clone())
            .or_default()
            .insert(store.into());
    }
    if !changed {
        return Ok(());
    }
    if !hidden.is_dir() {
        std::fs::create_dir(&hidden)?;
        basecraft_platform::files::sync_directory(folder)?;
    }
    let mut bytes = serde_json::to_vec_pretty(&index)?;
    bytes.push(b'\n');
    basecraft_io::write_atomic(&path, |w| w.write_all(&bytes))?;
    // The entry must be durable before the ledger it covers changes.
    basecraft_platform::files::sync_directory(&hidden)?;
    Ok(())
}

/// The stores listed for `source` in the index beside it.
pub(super) fn listed(source: &Path) -> Result<Vec<PathBuf>> {
    let source = lexical(source);
    let (Some(folder), Some(name)) = (source.parent(), source.file_name()) else {
        return Ok(Vec::new());
    };
    let index = read(&location(folder))?.unwrap_or_default();
    Ok(name
        .to_str()
        .and_then(|name| index.files.get(name))
        .into_iter()
        .flatten()
        .map(|store| lexical(&folder.join(store)))
        .collect())
}

/// Stores that can refer to `target`: those listed for it in the index beside
/// it under either spelling, and, for a file with several hard links, those
/// listed for any file there that is the same file. Stores that no longer exist
/// are left out: their ledgers went with them.
pub(super) fn select(
    target: &Path,
    absolute: &Path,
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
        let (Some(folder), Some(name)) = (spelling.parent(), spelling.file_name()) else {
            continue;
        };
        let path = location(folder);
        let index = match read(&path) {
            Ok(Some(index)) => index,
            Ok(None) => continue,
            Err(error) => {
                inspection.issue(super::InspectionCode::RegistryUnavailable, &path, error);
                continue;
            }
        };
        for (file, stores) in &index.files {
            if std::ffi::OsStr::new(file) == name || (linked && same_file(&folder.join(file))) {
                selected.extend(stores.iter().map(|store| lexical(&folder.join(store))));
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
        let _first = super::super::lock_save(&target, false, &[]).unwrap();
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
