//! Each analysis run's input record (`input.json`): which file the run read,
//! and what became of it when that file changed. Only known stores are
//! inspected, never filesystem trees.
mod check;
mod index;
mod locks;
mod package;
mod relink;
mod resolve;

pub(crate) use check::check_for;
pub use check::{
    AffectedAnalysis, EvidenceCheck, EvidenceConflict, InspectionCode, InspectionIssue,
    InspectionRelevance, SavePolicy, check, check_in, conflicts, conflicts_in,
};
pub(crate) use index::foreign_basecraft;
pub use locks::{BackgroundWait, Locks, lock, lock_in, wait_in_background};
pub(crate) use locks::{SaveLock, lock_save};
pub use package::package;
pub(crate) use relink::{ancestors, source_location};
pub use relink::{relink, verified_source, verify_ancestors};
pub(crate) use resolve::resolve_group;
pub use resolve::{preserve_input, resolve};

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use basecraft_protocol::input_records::{EvidenceState, Transformation};
use basecraft_protocol::provenance::{ArtifactRevision, FileStamp, InputReference, Publication};
use serde::Serialize;

use crate::{
    Error, Result,
    error::{damaged, invalid, unsupported},
    records::analysis_store::ArtifactRun,
};

/// Create a run whose input record references the original `source`, which
/// had `hash`, and whose history names the runs in `dependencies`. The source
/// is checked again under the record locks: by `captured_stamp`, the stamp it
/// had when `hash` was taken, or else by hashing. No source is copied. This is
/// an explicit analysis action, never part of opening a file.
pub fn reference_input(
    anchor: &Path,
    source: &Path,
    hash: &str,
    captured_stamp: Option<&FileStamp>,
    dependencies: &[PathBuf],
    origin: &str,
    project: Option<&crate::project::Project>,
) -> Result<ArtifactRun> {
    use basecraft_protocol::provenance::Completeness;
    let locks = lock_in(project)?;
    let run = ArtifactRun::create(project, anchor, &ulid::Ulid::generate().to_string())?;
    let source = source.canonicalize()?;
    locks.source(&source)?;
    if captured_stamp.map_or_else(
        || {
            basecraft_io::hash_file(&source)
                .map(|actual| actual != hash)
                .map_err(crate::Error::from)
        },
        |before| stamp(&source).map(|now| &now != before),
    )? {
        return Err(crate::Error::ChangedOnDisk { path: source });
    }
    let reference = InputReference {
        path: relative(run.directory(), &source)?,
        stamp: captured_stamp.cloned().map_or_else(|| stamp(&source), Ok)?,
        hash: hash.into(),
        extra: BTreeMap::new(),
    };
    // A referenced source inside the project keeps its history there.
    let source_bundle = crate::records::provenance::bundle_for_result(&source, project);
    let mut upstream = Vec::new();
    for dependency in dependencies.iter().chain(std::iter::once(&source_bundle)) {
        if let Some((record, hash)) = crate::records::provenance::latest(dependency)? {
            upstream.push(basecraft_protocol::input_records::UpstreamLink {
                bundle: (relative(run.directory(), dependency)?).into(),
                id: record.id,
                hash,
            });
        }
    }
    let input = SubmittedInput {
        schema_version: 1,
        source: Some(reference.clone()),
        submitted_hash: hash.into(),
        submitted_size: reference.stamp.size,
        transformation: basecraft_protocol::input_records::Transformation::Identity,
        state: EvidenceState::Referenced,
        analysis_run: None,
        proposal_target: None,
        upstream,
        origin_run: None,
        extra: BTreeMap::new(),
    };
    write_input(run.directory(), &input)?;
    run.write_run(
        basecraft_protocol::records::AnalysisRunStatus::Referenced,
        anchor
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned),
    )?;
    register(&run)?;
    let publication = Publication {
        schema_version: 1,
        id: run.id().into(),
        session: run.id().into(),
        parents: vec![],
        parent_hashes: BTreeMap::new(),
        created: basecraft_platform::now_rfc3339(),
        origin: origin.into(),
        inputs: vec![ArtifactRevision {
            id: format!("{}:source", run.id()),
            logical_name: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            hash: hash.into(),
            size: reference.stamp.size,
            retained: None,
            reference: Some(reference),
        }],
        outputs: vec![],
        events: vec![],
        effective_edits: vec![],
        withdrawn: Vec::new(),
        completeness: Completeness {
            ancestry_known: false,
            input_bytes_available: true,
            execution_specification_available: false,
            replay_verified: false,
        },
        report_template_version: 2,
    };
    crate::records::provenance::publish(run.directory(), &publication)?;
    Ok(run)
}

pub use basecraft_protocol::input_records::SubmittedInput;

pub fn stamp(path: &Path) -> Result<FileStamp> {
    stamp_metadata(path, &std::fs::metadata(path)?)
}

pub(crate) fn stamp_metadata(_path: &Path, metadata: &std::fs::Metadata) -> Result<FileStamp> {
    if !metadata.is_file() {
        return Err(crate::Error::Unsupported("input is not a file".into()));
    }
    let time = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| crate::Error::Unsupported("input timestamp is before Unix epoch".into()))?;
    #[cfg(unix)]
    let (device, inode) = {
        use std::os::unix::fs::MetadataExt;
        (Some(metadata.dev()), Some(metadata.ino()))
    };
    #[cfg(windows)]
    let (device, inode) = basecraft_platform::files::identity(_path)
        .map(|(device, inode)| (Some(device), Some(inode)))?;
    Ok(FileStamp {
        size: metadata.len(),
        modified_seconds: time.as_secs(),
        modified_nanos: time.subsec_nanos(),
        device,
        inode,
    })
}

/// Lexical relative path between canonical absolute locations, including parents.
/// Referenced paths are read-only; managed artifact names use stricter validation.
pub fn relative(base: &Path, target: &Path) -> Result<String> {
    let a: Vec<_> = base.components().collect();
    let b: Vec<_> = target.components().collect();
    let same = |x: &Component<'_>, y: &Component<'_>| {
        if x == y {
            return true;
        }
        #[cfg(windows)]
        if let (Component::Prefix(x), Component::Prefix(y)) = (x, y) {
            use std::path::Prefix::{Disk, UNC, VerbatimDisk, VerbatimUNC};
            return match (x.kind(), y.kind()) {
                (Disk(a) | VerbatimDisk(a), Disk(b) | VerbatimDisk(b)) => {
                    a.eq_ignore_ascii_case(&b)
                }
                (UNC(a, b) | VerbatimUNC(a, b), UNC(c, d) | VerbatimUNC(c, d)) => a == c && b == d,
                _ => false,
            };
        }
        false
    };
    let common = a.iter().zip(&b).take_while(|(x, y)| same(x, y)).count();
    if common == 0 {
        return target
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| unsupported("input path is not UTF-8"));
    }
    let mut path = PathBuf::new();
    for _ in common..a.len() {
        path.push("..");
    }
    for part in &b[common..] {
        path.push(part.as_os_str());
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| unsupported("input path is not UTF-8"))
}

pub fn read_input(run: &Path) -> Result<SubmittedInput> {
    locks::hold_record(run)?;
    let bytes = crate::records::provenance::read_record(&run.join("input.json"))?;
    validate_input(run, &bytes)
}

pub(super) fn validate_input(run: &Path, bytes: &[u8]) -> Result<SubmittedInput> {
    let metadata = std::fs::symlink_metadata(run.join("input.json"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(damaged(
            run.join("input.json"),
            "unsafe input evidence record",
        ));
    }
    let record: SubmittedInput = basecraft_protocol::records::decode(bytes)
        .map_err(|e| damaged(run.join("input.json"), e.to_string()))?;
    if record.schema_version != 1 {
        return Err(unsupported("unsupported input evidence record"));
    }
    let has_source = record.source.is_some();
    if match record.state {
        EvidenceState::Referenced | EvidenceState::Preserved => !has_source,
        EvidenceState::Generated => has_source,
        EvidenceState::Broken => false,
    } {
        return Err(damaged(
            run.join("input.json"),
            "inconsistent submitted input evidence",
        ));
    }
    Ok(record)
}

/// Write the record `value` to `path` through the one record encoder, which
/// names the basecraft version that wrote it.
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write_bytes(path, &basecraft_protocol::records::encode(value)?)
}

/// Write JSON basecraft keeps but does not author, unchanged: plugin data,
/// plain values and lists inside a run folder, whose run record names the
/// writing version.
pub fn write_plain_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_bytes(path, &bytes)
}

pub(super) fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    Ok(basecraft_io::write_atomic(path, |w| w.write_all(bytes))?)
}

/// Write the `input.json` of the run in `directory`, holding its record and
/// every source it depends on locked for the rest of the scope, so no save
/// replaces them meanwhile. When what it links to changes (its source or the
/// runs it names), its store's analysis index lists the run under every
/// source it now depends on, and so do the runs elsewhere that were listed
/// under its old source.
pub fn write_input(directory: &Path, input: &SubmittedInput) -> Result<()> {
    let path = directory.join("input.json");
    let bytes = basecraft_protocol::records::encode(input)?;
    if let Some(store) = index::store_of(&path) {
        // Lock order: the ledger and its inputs, then each index briefly.
        let locks = lock()?;
        let mut sources = std::collections::BTreeSet::new();
        locks.input(
            directory,
            input,
            &mut std::collections::BTreeSet::new(),
            &mut sources,
        )?;
        let old = read_input(directory).ok();
        let links = |input: &SubmittedInput| {
            (
                input.source.clone(),
                input.analysis_run.clone(),
                input.upstream.clone(),
                input.origin_run.clone(),
            )
        };
        if old.as_ref().is_none_or(|old| links(old) != links(input)) {
            let project = locks.project();
            let project = project.as_deref();
            index::add(store, &sources, project)?;
            if let Some(reference) = old.as_ref().and_then(|old| old.source.as_ref()) {
                let previous = directory.join(&reference.path);
                for dependent in index::listed(&previous, project)? {
                    index::add(&dependent, &sources, project)?;
                }
            }
        }
    }
    basecraft_io::write_atomic(&path, |w| w.write_all(&bytes))?;
    basecraft_platform::files::sync_directory(directory)?;
    Ok(())
}

/// Find an input run by identity in the folder tree whose `.basecraft` holds
/// `near`, or else in the folder holding `near`: the project, or the folder
/// of a loose file. This keeps machine paths out of public `ToolRun` parameters.
pub(crate) fn registered_run(id: &str, expected: &str, near: &Path) -> Result<PathBuf> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(invalid("Unsafe input run identity"));
    }
    let near = std::path::absolute(near)?;
    let root = near
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == ".basecraft"))
        .and_then(Path::parent)
        .or_else(|| near.parent())
        .unwrap_or(&near);
    let mut found = None;
    for store in stores_under(root)? {
        let run = store.join("runs").join(id);
        let record = run.join("records").join(format!("{id}.json"));
        if !record.try_exists()? {
            continue;
        }
        ArtifactRun::validate_store(&store)?;
        for directory in [store.join("runs"), run.clone()] {
            crate::records::analysis_store::ordinary_folder(
                &directory,
                "Unsafe registered input run",
            )?;
        }
        if basecraft_io::hash_file(&record)? != expected {
            return Err(damaged(record, "Registered input run checksum differs"));
        }
        let canonical = run.canonicalize()?;
        if found.as_ref().is_some_and(|prior| prior != &canonical) {
            return Err(Error::NotReady(
                "Input run has multiple registered copies; explicit reassociation required".into(),
            ));
        }
        found = Some(canonical);
    }
    found.ok_or_else(|| invalid("Selected input run unavailable in known stores"))
}

/// Every `.basecraft/analyses-v1` below `root`, skipping hidden folders and
/// symbolic links. Bounded, since `root` may be a large loose-file folder.
fn stores_under(root: &Path) -> Result<Vec<PathBuf>> {
    let mut stores = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    let mut visited = 0;
    while let Some(folder) = pending.pop() {
        visited += 1;
        if visited > 20_000 {
            return Err(crate::Error::Limit {
                what: "folders searched for an input run".into(),
                limit: 20_000,
            });
        }
        let store = folder.join(".basecraft/analyses-v1");
        if store.is_dir() {
            stores.push(store);
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir())
                && !entry.file_name().to_string_lossy().starts_with('.')
            {
                pending.push(entry.path());
            }
        }
    }
    Ok(stores)
}

/// What `reindex` rebuilt, and the ledgers it could not read.
#[derive(Debug, Serialize)]
pub struct Reindexed {
    pub stores: usize,
    pub folders: usize,
    pub unreadable: Vec<PathBuf>,
}

/// Rebuild the analysis index of every folder that the stores in `project`
/// use, from the stores' ledgers: the repair for a damaged or lost index.
/// A store with an unreadable ledger only gains entries, since that ledger's
/// inputs are unknown.
pub fn reindex(project: &Path) -> Result<Reindexed> {
    let project = project.canonicalize()?;
    let locks = lock()?;
    locks.set_project(Some(&project));
    let mut done = Reindexed {
        stores: 0,
        folders: 0,
        unreadable: Vec::new(),
    };
    for store in stores_under(&project)? {
        ArtifactRun::validate_store(&store)?;
        let mut unreadable = Vec::new();
        done.folders += index::rebuild(&store, Some(&project), || {
            let mut sources = std::collections::BTreeSet::new();
            for entry in std::fs::read_dir(store.join("runs"))? {
                let run = entry?.path();
                if !run.join("input.json").is_file() {
                    continue;
                }
                if let Ok(input) = read_input(&run) {
                    locks.input(
                        &run,
                        &input,
                        &mut std::collections::BTreeSet::new(),
                        &mut sources,
                    )?;
                } else {
                    unreadable.push(run.join("input.json"));
                }
            }
            Ok((sources, unreadable.is_empty()))
        })?;
        done.unreadable.extend(unreadable);
        done.stores += 1;
    }
    Ok(done)
}

/// Caller holds `lock`. List the run's store beside every input its ledger
/// names, directly or through the runs it depends on.
pub fn register(run: &ArtifactRun) -> Result<()> {
    let store = run.store();
    let ledger = run.directory().join("input.json");
    if !ledger.try_exists()? {
        return Ok(());
    }
    let input = read_input(run.directory())?;
    let mut sources = std::collections::BTreeSet::new();
    let locks = lock()?;
    locks.input(
        run.directory(),
        &input,
        &mut std::collections::BTreeSet::new(),
        &mut sources,
    )?;
    index::add(store, &sources, locks.project().as_deref())
}

/// Write the input record of `run`, which reads generated `payload` rather
/// than a file, linked to the histories in `dependencies`, and its run record.
/// The caller keeps the payload as a published artifact and registers the run.
pub fn link_generated_input(
    run: &ArtifactRun,
    dependencies: &[PathBuf],
    payload: &[u8],
    transformation: Transformation,
    result_label: &str,
) -> Result<()> {
    let mut upstream = Vec::new();
    for dependency in dependencies {
        let (publication, hash) = crate::records::provenance::latest_record(dependency)?;
        upstream.push(basecraft_protocol::input_records::UpstreamLink {
            bundle: (relative(run.directory(), dependency)?).into(),
            id: publication.id,
            hash,
        });
    }
    write_input(
        run.directory(),
        &SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: basecraft_io::hash_bytes(payload),
            submitted_size: payload.len() as u64,
            transformation,
            state: EvidenceState::Generated,
            analysis_run: None,
            proposal_target: None,
            upstream,
            origin_run: None,
            extra: BTreeMap::new(),
        },
    )?;
    run.write_run(
        basecraft_protocol::records::AnalysisRunStatus::Prepared,
        Some(result_label.into()),
    )
}

#[cfg(all(test, windows))]
mod windows_paths {
    #[test]
    fn relative_paths_compare_windows_volume_spellings_without_io() {
        use std::path::Path;
        assert_eq!(
            super::relative(
                Path::new(r"\\?\C:\work\package"),
                Path::new(r"C:\work\package\artifacts\data")
            )
            .unwrap(),
            r"artifacts\data"
        );
        assert_eq!(
            super::relative(
                Path::new(r"\\?\UNC\server\share\base"),
                Path::new(r"\\server\share\other")
            )
            .unwrap(),
            r"..\other"
        );
        assert_eq!(
            super::relative(Path::new(r"C:\base"), Path::new(r"D:\target")).unwrap(),
            r"D:\target"
        );
    }
}
