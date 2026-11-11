//! Referenced-input lifecycle. Only known stores are inspected, never filesystem trees.
mod index;

use crate::records::links::AlignmentLinkExt;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use basecraft_protocol::export_records::{ProvenanceExportMap, ProvenanceExportMapping};
use basecraft_protocol::provenance::{FileStamp, InputReference};
use basecraft_protocol::records::StoreRegistry;
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

use crate::{Result, error::invalid, records::analysis_store::ArtifactRun};

/// Register one clean original using the shared reference/preserve/export lifecycle.
/// This is an explicit analysis action, never a passive-open writer. Dependencies
/// name immutable upstream runs; no source input is copied here.
pub fn reference_input_with_dependencies(
    anchor: &Path,
    source: &Path,
    hash: &str,
    dependencies: &[PathBuf],
    origin: &str,
) -> Result<ArtifactRun> {
    Mutation::begin()?.reference_input_with_dependencies(anchor, source, hash, dependencies, origin)
}

/// One cooperating evidence mutation boundary. Keep this value alive through
/// related ledger/ownership writes; never call a locking wrapper while held.
/// Files remain separate atomic writes: failure may leave conservative partial
/// evidence, and dropping this guard does not roll those writes back.
pub struct Mutation {
    _locks: Locks,
}

impl Mutation {
    pub fn begin() -> Result<Self> {
        Ok(Self { _locks: lock()? })
    }

    pub fn read_input(&self, directory: &Path) -> Result<SubmittedInput> {
        read_input(directory)
    }

    pub fn write_input(&self, directory: &Path, input: &SubmittedInput) -> Result<()> {
        write_json(&directory.join("input.json"), input)
    }

    /// Preserve one owned leaf without reacquiring the evidence lock.
    pub fn preserve_input(&self, bundle: &Path) -> Result<()> {
        preserve_input_locked(bundle)
    }

    pub fn register(&self, run: &ArtifactRun) -> Result<()> {
        register(run)
    }

    pub fn link_generated_input(
        &self,
        run: &ArtifactRun,
        dependencies: &[PathBuf],
        payload: &[u8],
        transformation: &str,
        result_label: &str,
    ) -> Result<()> {
        link_generated_input_locked(
            run,
            dependencies,
            payload,
            transformation,
            result_label,
            register,
        )
    }

    /// Recheck the caller's frozen original hash under the held evidence lock.
    pub fn reference_input_with_dependencies(
        &self,
        anchor: &Path,
        source: &Path,
        hash: &str,
        dependencies: &[PathBuf],
        origin: &str,
    ) -> Result<ArtifactRun> {
        reference_input_locked(anchor, source, hash, dependencies, origin, None)
    }
}

pub(crate) fn reference_input_locked(
    anchor: &Path,
    source: &Path,
    hash: &str,
    dependencies: &[PathBuf],
    origin: &str,
    captured_stamp: Option<&FileStamp>,
) -> Result<ArtifactRun> {
    use basecraft_protocol::provenance::{
        ArtifactRevision, Completeness, InputReference, Publication,
    };
    let run = crate::records::analysis_store::ArtifactRun::create(
        anchor,
        &ulid::Ulid::generate().to_string(),
    )?;
    let source = source.canonicalize()?;
    let reservation = lock()?;
    reservation.source(&source)?;
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
        path: crate::records::input_evidence::relative(run.directory(), &source)?,
        stamp: captured_stamp.cloned().map_or_else(|| stamp(&source), Ok)?,
        hash: hash.into(),
        extra: BTreeMap::new(),
    };
    let mut input = crate::records::input_evidence::SubmittedInput {
        schema_version: 1,
        source: Some(reference.clone()),
        submitted_hash: hash.into(),
        submitted_size: reference.stamp.size,
        transformation: "identity-v1".into(),
        state: "referenced".into(),
        analysis_run: None,
        proposal_target: None,
        upstream: Vec::new(),
        origin_run: None,
        extra: BTreeMap::new(),
    };
    let source_bundle = crate::records::provenance::bundle_for_result(&source, None);
    let mut upstream = Vec::new();
    for dependency in dependencies.iter().chain(std::iter::once(&source_bundle)) {
        let manifest = dependency.join("manifest.json");
        if manifest.is_file() {
            let bytes = crate::records::provenance::read_record(&manifest)?;
            let record: Publication = serde_json::from_slice(&bytes)?;
            upstream.push(basecraft_protocol::input_records::UpstreamLink {
                bundle: (crate::records::input_evidence::relative(run.directory(), dependency)?)
                    .into(),
                id: record.id.clone(),
                hash: basecraft_io::hash_bytes(&bytes),
            });
        }
    }
    if !upstream.is_empty() {
        input.upstream = upstream;
    }
    crate::records::input_evidence::write_json(&run.artifact("input.json")?, &input)?;
    run.write_run(
        basecraft_protocol::records::AnalysisRunStatus::Referenced,
        anchor
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned),
    )?;
    crate::records::input_evidence::register(&run)?;
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
        completeness: Completeness {
            ancestry_known: false,
            input_bytes_available: true,
            execution_specification_available: false,
            replay_verified: false,
        },
        report_template_version: 2,
    };
    crate::records::provenance::publish(run.directory(), &publication)?;
    // Publication can refresh the input ledger after its initial registration.
    index::register(run.directory().parent().unwrap().parent().unwrap(), None)?;
    Ok(run)
}

pub use basecraft_protocol::input_records::SubmittedInput;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavePolicy {
    #[default]
    Cancel,
    PreserveCopy,
    RemoveAnalysis,
    BreakEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AffectedAnalysis {
    pub run: PathBuf,
    pub result: String,
    pub source_hash: String,
    pub can_preserve: bool,
    pub direct: bool,
    /// Backing decisions for a logical analysis; empty for a direct ledger.
    /// Each child retains its own hash/direct guard.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backing_analyses: Vec<AffectedAnalysis>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceConflict {
    pub target: PathBuf,
    pub analyses: Vec<AffectedAnalysis>,
    pub options: Vec<SavePolicy>,
}

/// Failure of an inspection, never authority to ignore a save/write failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionCode {
    RegistryUnavailable,
    StoreUnavailable,
    RunUnavailable,
    LedgerInvalid,
    DependencyUnavailable,
    IdentityUnverified,
    DisplayMetadataUnavailable,
    LimitExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionRelevance {
    Relevant,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionIssue {
    pub code: InspectionCode,
    pub location: PathBuf,
    pub relevance: InspectionRelevance,
    /// No distance-based unrelatedness inference is made. An unreadable record
    /// can hide a direct reference, an alias, an owner or an ancestor edge.
    pub reason: String,
}

/// Advisory snapshot. It performs no registration, preservation or writes.
#[derive(Clone, Debug, Serialize)]
pub struct EvidenceCheck {
    pub conflict: EvidenceConflict,
    pub issues: Vec<InspectionIssue>,
    /// Ephemeral digest of inspected inventory, including damaged bytes.
    pub inventory_revision: String,
}

impl EvidenceCheck {
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Default)]
struct Inspection {
    observations: BTreeMap<PathBuf, String>,
    issues: Vec<InspectionIssue>,
}

impl Inspection {
    fn issue(&mut self, code: InspectionCode, location: &Path, error: impl std::fmt::Display) {
        self.issue_for(code, InspectionRelevance::Unknown, location, error);
    }

    fn issue_for(
        &mut self,
        code: InspectionCode,
        relevance: InspectionRelevance,
        location: &Path,
        error: impl std::fmt::Display,
    ) {
        self.issues.push(InspectionIssue {
            code,
            location: location.into(),
            relevance,
            reason: error.to_string(),
        });
    }

    fn record(&mut self, path: &Path) -> Result<Vec<u8>> {
        self.record_bounded(path, 64 << 20)
    }

    fn record_bounded(&mut self, path: &Path, mut limit: u64) -> Result<Vec<u8>> {
        let result = crate::records::provenance::read_record_with_budget(path, &mut limit);
        self.observe(path, &result);
        result
    }

    fn observe(&mut self, path: &Path, result: &Result<Vec<u8>>) {
        self.observations.insert(
            path.into(),
            match result {
                Ok(bytes) => basecraft_io::hash_bytes(bytes),
                Err(error) => format!("unavailable: {error}"),
            },
        );
    }

    fn input(&mut self, run: &Path) -> Result<SubmittedInput> {
        // Keep the existing unsafe-record and schema validation in one owner.
        let bytes = self.record(&run.join("input.json"))?;
        validate_input(run, &bytes)
    }

    fn name(&mut self, run: &Path) -> String {
        let path = run.join("run.json");
        let result = self.record(&path).and_then(|bytes| {
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            Ok(value["result"]
                .as_str()
                .unwrap_or("Unpublished analysis")
                .to_owned())
        });
        match result {
            Ok(name) => name,
            Err(error) => {
                self.issue_for(
                    InspectionCode::DisplayMetadataUnavailable,
                    InspectionRelevance::Relevant,
                    &path,
                    error,
                );
                "Unpublished analysis".into()
            }
        }
    }
}

impl std::fmt::Display for EvidenceConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // CLI/SDK callers receive a structured conflict without an interactive prompt.
        write!(
            f,
            "analysis_input_conflict: {}",
            serde_json::to_string(self).map_err(|_| std::fmt::Error)?
        )
    }
}

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
            .ok_or_else(|| invalid("input path is not UTF-8"));
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
        .ok_or_else(|| invalid("input path is not UTF-8"))
}

pub fn read_input(run: &Path) -> Result<SubmittedInput> {
    if let Some(held) = HELD_LOCKS.with(|slot| slot.borrow().upgrade()) {
        Locks(held).record(run)?;
    }
    let bytes = crate::records::provenance::read_record(&run.join("input.json"))?;
    validate_input(run, &bytes)
}

fn validate_input(run: &Path, bytes: &[u8]) -> Result<SubmittedInput> {
    let metadata = std::fs::symlink_metadata(run.join("input.json"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("unsafe input evidence record"));
    }
    let record: SubmittedInput = serde_json::from_slice(bytes)?;
    if record.schema_version != 1
        || !matches!(
            record.state.as_str(),
            "referenced" | "generated" | "preserved" | "broken"
        )
    {
        return Err(invalid("unsupported input evidence record"));
    }
    if !matches!(
        record.transformation.as_str(),
        "canonical-fasta-v1"
            | "ungapped-fasta-v1"
            | "canonical-newick-v1"
            | "identity-v1"
            | "blast-query-preparation-v1"
            | "sequence-arrow-buffers-v1"
    ) || matches!(record.state.as_str(), "referenced" | "preserved") && record.source.is_none()
        || record.state == basecraft_protocol::input_records::EvidenceState::Generated
            && record.source.is_some()
    {
        return Err(invalid(
            "unsupported or inconsistent submitted input evidence",
        ));
    }
    Ok(record)
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if path.file_name().is_some_and(|name| name == "input.json") {
        let locks = lock()?;
        let mut sources = Some(std::collections::BTreeSet::new());
        if let Ok(input) = serde_json::from_slice::<SubmittedInput>(&bytes) {
            locks.input(
                path.parent().unwrap(),
                &input,
                &mut std::collections::BTreeSet::new(),
                &mut sources,
            )?;
        } else {
            sources = None;
        }
        // Lock order: the ledger and its inputs first, then the catalogue.
        let guard = registry_lock()?;
        // An ancestry change cannot be scoped until its index is published.
        // Keep it atomic to lookups; direct-source updates need no long lock.
        let ancestry = sources.is_none();
        let store = index::changing(path, sources)?;
        let guard = if ancestry {
            Some(guard)
        } else {
            drop(guard);
            None
        };
        basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
        basecraft_platform::files::sync_directory(path.parent().unwrap())?;
        if let Some(store) = store {
            // A damaged sibling must not prevent an explicitly acknowledged
            // evidence decision. The durable dirty marker keeps coverage unknown.
            let _ = index::register(&store, guard);
        }
    } else {
        basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
    }
    Ok(())
}

fn registry() -> Result<PathBuf> {
    let paths =
        basecraft_platform::paths::AppPaths::discover().map_err(|e| invalid(e.to_string()))?;
    Ok(registry_location(&paths))
}

pub(crate) fn registry_location(paths: &basecraft_platform::paths::AppPaths) -> PathBuf {
    paths.state.join("analysis-stores.json")
}

fn open_lock(path: &Path) -> Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?)
}

/// Reserved keys, each with whether it is held exclusively. Ledgers (run
/// folders) are always reserved exclusively, reads included, since a read is
/// often followed by a write. Store keys are only taken for store-level
/// changes and by saves, up front.
#[derive(Default)]
struct Scope {
    held: std::cell::RefCell<BTreeMap<PathBuf, (std::fs::File, bool)>>,
}
type HeldLocks = std::rc::Rc<Scope>;
thread_local! {
    static HELD_LOCKS: std::cell::RefCell<std::rc::Weak<Scope>> = const { std::cell::RefCell::new(std::rc::Weak::new()) };
}
const RESERVATION_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// A thread-confined mutation scope. Readers reserve shared, writers exclusive;
/// nested operations reuse the scope. Cooperating writers hold keys briefly, so
/// a reservation waits up to 2 s; contention then ends in Busy, never deadlock.
pub struct Locks(HeldLocks);

pub fn lock() -> Result<Locks> {
    Ok(HELD_LOCKS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let held = slot.upgrade().unwrap_or_default();
        *slot = std::rc::Rc::downgrade(&held);
        Locks(held)
    }))
}

impl Locks {
    fn record(&self, directory: &Path) -> Result<()> {
        let directory = lexical(&std::path::absolute(directory)?);
        self.reserve([directory], true)
    }
    fn input(
        &self,
        directory: &Path,
        input: &SubmittedInput,
        seen: &mut std::collections::BTreeSet<PathBuf>,
        sources: &mut Option<std::collections::BTreeSet<PathBuf>>,
    ) -> Result<()> {
        self.record(directory)?;
        if !seen.insert(directory.into()) {
            return Ok(());
        }
        if seen.len() > 32 {
            return Err(crate::Error::Limit {
                what: "input ancestry".into(),
                limit: 32,
            });
        }
        if let Some(source) = &input.source {
            let path = lexical(&directory.join(&source.path));
            self.source(&path)?;
            if let Some(sources) = sources {
                sources.extend(index::spellings(&path));
            }
        }
        let mut dependencies = Vec::new();
        let origin = input.extra.get("origin_run").map(|value| value.as_str());
        if origin.is_some() || input.analysis_run.is_some() || !input.upstream.is_empty() {
            // Unpublished dependency edges can acquire new sources. Only direct
            // references have stable pending coverage.
            *sources = None;
        }
        if origin == Some(None) {
            return Err(invalid("invalid origin_run"));
        }
        dependencies.extend(origin.flatten().map(Path::new));
        dependencies.extend(input.analysis_run.as_deref().map(Path::new));
        dependencies.extend(input.upstream.iter().map(|link| link.bundle.as_path()));
        for relative in dependencies {
            let path = lexical(&directory.join(relative));
            match read_input(&path) {
                Ok(input) => self.input(&path, &input, seen, sources)?,
                Err(error @ crate::Error::Busy(_)) => return Err(error),
                Err(_) => *sources = None,
            }
        }
        Ok(())
    }
    fn reserve(&self, paths: impl IntoIterator<Item = PathBuf>, exclusive: bool) -> Result<()> {
        let mut held = self.0.held.borrow_mut();
        for path in paths.into_iter().collect::<std::collections::BTreeSet<_>>() {
            let path = if path.is_absolute() {
                path.canonicalize().unwrap_or(path)
            } else {
                path
            };
            if let Some((_, held_exclusive)) = held.get(&path) {
                // flock conversion is not atomic: never upgrade a shared hold.
                // A scope that will write reserves exclusively up front.
                if exclusive && !*held_exclusive {
                    return Err(crate::Error::Busy(format!(
                        "this save did not reserve {} before changing it",
                        path.display()
                    )));
                }
                continue;
            }
            let key = basecraft_io::hash_bytes(path.as_os_str().as_encoded_bytes());
            let file = open_lock(
                &registry()?
                    .with_file_name("analysis-locks")
                    .join(key.trim_start_matches("b3:")),
            )?;
            Self::acquire(&file, exclusive, &path)?;
            held.insert(path, (file, exclusive));
        }
        Ok(())
    }
    fn acquire(file: &std::fs::File, exclusive: bool, path: &Path) -> Result<()> {
        let started = std::time::Instant::now();
        loop {
            let acquired = if exclusive {
                file.try_lock_exclusive()?
            } else {
                FileExt::try_lock_shared(file)?
            };
            if acquired {
                return Ok(());
            }
            if started.elapsed() > RESERVATION_WAIT {
                return Err(crate::Error::Busy(format!(
                    "an analysis or save is using {}; retry shortly",
                    path.display()
                )));
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    pub(crate) fn source(&self, path: &Path) -> Result<()> {
        let absolute = lexical(&std::path::absolute(path)?);
        let mut keys = vec![absolute.clone()];
        if let Ok(canonical) = absolute.canonicalize() {
            keys.push(canonical);
        }
        if let Ok(meta) = stamp(path)
            && let (Some(dev), Some(ino)) = (meta.device, meta.inode)
        {
            keys.push(PathBuf::from(format!("inode:{dev}:{ino}")));
        }
        self.reserve(keys, true)
    }
}

fn registry_lock() -> Result<std::fs::File> {
    let lock = open_lock(&registry()?.with_file_name("analysis-registry.lock"))?;
    lock.lock_exclusive()
        .map_err(|e| crate::Error::Busy(e.to_string()))?;
    Ok(lock)
}

pub(crate) struct SaveLock {
    _locks: Locks,
    check: Option<EvidenceCheck>,
}

impl SaveLock {
    pub(crate) fn check(&mut self, target: &Path) -> Result<EvidenceCheck> {
        self.check.take().map_or_else(|| check(target), Ok)
    }
}

/// Select and reserve under the short catalogue lock, then retain only resources
/// belonging to this target. A lookup is carried into the save unchanged.
/// `writes` are stores this save will change (its history bundle's store);
/// they are reserved exclusively first. Other selected stores are reserved
/// shared unless `changes_stores` (a policy that edits ledgers) is set.
pub(crate) fn lock_save(
    target: &Path,
    changes_stores: bool,
    writes: &[PathBuf],
) -> Result<SaveLock> {
    let locks = lock()?;
    // Lock order: reservations first, then the catalogue, which is never held
    // while waiting. Holding the target's keys stops any new reference to it,
    // so stores selected under the shared catalogue stay the right set.
    locks.reserve(writes.iter().cloned(), true)?;
    locks.source(target)?;
    let (canonical, selected, inspection) = {
        let catalogue = open_lock(&registry()?.with_file_name("analysis-registry.lock"))?;
        FileExt::lock_shared(&catalogue).map_err(|e| crate::Error::Busy(e.to_string()))?;
        selection(target)?
    };
    locks.reserve(selected.iter().cloned(), changes_stores)?;
    let check = inspect(&canonical, &selected, inspection)?;
    Ok(SaveLock {
        _locks: locks,
        check: Some(check),
    })
}

fn empty_registry() -> StoreRegistry {
    StoreRegistry {
        schema_version: 1,
        stores: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn parse_store_registry(bytes: &[u8]) -> Result<StoreRegistry> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    if value["schema_version"] != 1 || !value["stores"].is_array() {
        return Err(invalid("unsupported known-store registry"));
    }
    Ok(serde_json::from_value(value)?)
}

fn store_registry() -> Result<StoreRegistry> {
    let path = registry()?;
    match std::fs::File::open(&path) {
        Ok(file) => {
            let length = file.metadata()?.len();
            let bytes = crate::records::provenance::read_record_from_open(
                file,
                length,
                &mut (1 << 20),
                1 << 20,
                crate::records::provenance::RecordLimit::Registry,
            )?;
            #[cfg(feature = "test-counters")]
            basecraft_io::counters::record_read(&path, bytes.len() as u64);
            parse_store_registry(&bytes)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(empty_registry()),
        Err(e) => Err(e.into()),
    }
}

fn stores() -> Result<Vec<PathBuf>> {
    let paths = store_registry()?.stores;
    validate_store_paths(&paths)?;
    Ok(paths)
}

fn validate_store_paths(paths: &[PathBuf]) -> Result<()> {
    if paths.len() > 4096 || paths.iter().any(|p| !p.is_absolute()) {
        return Err(invalid("known-store paths must be absolute and bounded"));
    }
    Ok(())
}

/// Resolve an immutable run by identity only in registered stores. This avoids
/// exposing machine paths in public `ToolRun` parameters and never scans trees.
pub(crate) fn registered_run(id: &str, expected: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(invalid("Unsafe input run identity"));
    }
    let mut found = None;
    for store in stores()? {
        let run = store.join("runs").join(id);
        let record = run.join("records").join(format!("{id}.json"));
        if !record.try_exists()? {
            continue;
        }
        ArtifactRun::validate_store(&store)?;
        for directory in [store.join("runs"), run.clone()] {
            let metadata = std::fs::symlink_metadata(directory)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(invalid("Unsafe registered input run"));
            }
        }
        if basecraft_io::hash_file(&record)? != expected {
            return Err(invalid("Registered input run checksum differs"));
        }
        let canonical = run.canonicalize()?;
        if found.as_ref().is_some_and(|prior| prior != &canonical) {
            return Err(invalid(
                "Input run has multiple registered copies; explicit reassociation required",
            ));
        }
        found = Some(canonical);
    }
    found.ok_or_else(|| invalid("Selected input run unavailable in known stores"))
}

/// Caller holds `lock`. Only explicit analysis execution registers a store.
pub fn register(run: &ArtifactRun) -> Result<()> {
    let store = run
        .directory()
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| invalid("run has no store"))?;
    if index::is_private(store) {
        return Ok(());
    }
    // The catalogue lock serialises index publication. Reserving the store
    // here too would order store before inputs, the reverse of a writer that
    // reserves its inputs first, and the two would wait on each other.
    let registry_guard = registry_lock()?;
    let mut all = stores()?;
    if !all.iter().any(|p| p == store) {
        if all.len() >= 4096 {
            return Err(invalid("known-store registry limit reached"));
        }
        all.push(store.into());
        let mut value = store_registry()?;
        value.stores = all;
        write_json(&registry()?, &serde_json::to_value(value)?)?;
    }
    drop(registry_guard);
    index::register(store, None)?;
    Ok(())
}

fn same_source(run: &Path, reference: &InputReference, target: &Path) -> bool {
    let source = run.join(&reference.path);
    if source.canonicalize().ok().as_deref() == Some(target) {
        return true;
    }
    if let (Ok(a), Ok(b)) = (stamp(&source), stamp(target)) {
        return a.device.is_some() && a.device == b.device && a.inode == b.inode;
    }
    // Missing inputs still match their lexical old location.
    lexical(&source) == lexical(target)
}

fn lexical(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            _ => result.push(part),
        }
    }
    result
}

/// Metadata-only UI query. Matching metadata is a signal, never a hash verdict.
pub fn conflicts(target: &Path) -> Result<EvidenceConflict> {
    conservative(check(target)?)
}

fn conservative(check: EvidenceCheck) -> Result<EvidenceConflict> {
    if !check.is_complete() {
        return Err(crate::Error::EvidenceCheckIncomplete(Box::new(check)));
    }
    Ok(check.conflict)
}

/// Side-effect-free full inspection; callers must recheck under `lock` before
/// acting. Failure of the lock or of an actual write is never an inspection issue.
pub fn check(target: &Path) -> Result<EvidenceCheck> {
    // Advisory inspection creates no profile state. Writers create the lock
    // before their first index change; an empty profile needs no reservation.
    let catalogue = match std::fs::File::open(registry()?.with_file_name("analysis-registry.lock"))
    {
        Ok(file) => {
            FileExt::lock_shared(&file).map_err(|e| crate::Error::Busy(e.to_string()))?;
            Some(file)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let (canonical, selected, inspection) = selection(target)?;
    drop(catalogue);
    inspect(&canonical, &selected, inspection)
}

fn selection(target: &Path) -> Result<(PathBuf, Vec<PathBuf>, Inspection)> {
    let mut inspection = Inspection::default();
    let absolute = std::path::absolute(target)?;
    let canonical = absolute.canonicalize().unwrap_or_else(|_| absolute.clone());
    let mut nearby = Vec::new();
    for parent in canonical.ancestors().skip(1) {
        let store = parent.join(".basecraft/analyses-v1");
        match store.try_exists() {
            Ok(true) => nearby.push(store),
            Err(error) => inspection.issue(InspectionCode::StoreUnavailable, &store, error),
            _ => {}
        }
    }
    let selected = match index::select(&canonical, &absolute, &nearby, &mut inspection) {
        Ok(selected) => selected,
        Err(error) => {
            inspection.issue(InspectionCode::RegistryUnavailable, &registry()?, error);
            nearby
        }
    };
    Ok((canonical, selected, inspection))
}

#[derive(Default)]
struct StoreScan {
    runs: Vec<(PathBuf, SubmittedInput)>,
    analyses: Vec<AffectedAnalysis>,
    metadata_bytes: u64,
}

fn scan_store(store: &Path, target: &Path, scan: &mut StoreScan, inspection: &mut Inspection) {
    #[cfg(feature = "test-counters")]
    basecraft_io::counters::record_store_inspection(store);
    // The marker can be damaged in different ways with the same parse error.
    // Its observed bytes participate in consent, even when validation fails.
    let _ = inspection.record_bounded(&store.join("store.json"), 4096);
    let result = ArtifactRun::validate_store(store)
        .and_then(|()| Ok(std::fs::read_dir(store.join("runs"))?));
    let entries = match result {
        Ok(entries) => entries,
        Err(error) => {
            inspection.issue(InspectionCode::StoreUnavailable, store, error);
            return;
        }
    };
    for (count, entry) in entries.enumerate() {
        if count >= 4096 {
            inspection.issue(
                InspectionCode::LimitExceeded,
                store,
                "store exceeds 4096 runs; explicit inspection required",
            );
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                inspection.issue(InspectionCode::RunUnavailable, &store.join("runs"), error);
                continue;
            }
        };
        let run = entry.path();
        match scan_run(&entry, target, &mut scan.metadata_bytes, inspection) {
            Ok(Some((input, analysis, bytes))) => {
                scan.metadata_bytes = bytes;
                scan.runs.push((run, input));
                scan.analyses.extend(analysis);
            }
            Ok(None) => {}
            Err(error) => inspection.issue(
                InspectionCode::LedgerInvalid,
                &run.join("input.json"),
                error,
            ),
        }
    }
}

type RunScan = (SubmittedInput, Option<AffectedAnalysis>, u64);

fn scan_run(
    entry: &std::fs::DirEntry,
    target: &Path,
    metadata_bytes: &mut u64,
    inspection: &mut Inspection,
) -> Result<Option<RunScan>> {
    let kind = entry.file_type()?;
    if kind.is_file() {
        return Ok(None);
    }
    if !kind.is_dir() {
        return Err(crate::Error::Damaged {
            path: entry.path(),
            detail: "unsafe entry in analysis store".into(),
        });
    }
    let run = entry.path();
    if !run.join("input.json").try_exists()? {
        return Ok(None); // Reserved, no input registered yet.
    }
    let bytes = metadata_bytes.saturating_add(std::fs::metadata(run.join("input.json"))?.len());
    *metadata_bytes = bytes;
    if bytes > 64 << 20 {
        inspection.issue(
            InspectionCode::LimitExceeded,
            &run.join("input.json"),
            "known analysis metadata exceeds 64 MiB query limit",
        );
        return Ok(None);
    }
    let input = inspection.input(&run)?;
    let mut analysis = None;
    let proposal_target = input.proposal_target == Some(true);
    let source_matches = if input.state
        == basecraft_protocol::input_records::EvidenceState::Referenced
        && !proposal_target
    {
        input.source.as_ref().is_some_and(|reference| {
            if same_source(&run, reference, target) {
                return true;
            }
            let source = run.join(&reference.path);
            if let Err(error) = source
                .canonicalize()
                .map_err(crate::Error::from)
                .and_then(|_| stamp(&source))
            {
                inspection.issue(InspectionCode::IdentityUnverified, &source, error);
            }
            false
        })
    } else {
        false
    };
    if source_matches && let Some(reference) = &input.source {
        let can_preserve = stamp(target).is_ok_and(|s| {
            s.size == reference.stamp.size
                && s.modified_seconds == reference.stamp.modified_seconds
                && s.modified_nanos == reference.stamp.modified_nanos
        });
        analysis = Some(AffectedAnalysis {
            result: inspection.name(&run),
            run,
            source_hash: reference.hash.clone(),
            can_preserve,
            direct: true,
            backing_analyses: vec![],
        });
    }
    Ok(Some((input, analysis, bytes)))
}

pub fn conflicts_in(target: &Path, stores: &[PathBuf]) -> Result<EvidenceConflict> {
    conservative(check_in(target, stores)?)
}

pub fn check_in(target: &Path, stores: &[PathBuf]) -> Result<EvidenceCheck> {
    inspect(target, stores, Inspection::default())
}

fn inspect(target: &Path, stores: &[PathBuf], mut inspection: Inspection) -> Result<EvidenceCheck> {
    let mut scan = StoreScan::default();
    for store in stores {
        scan_store(store, target, &mut scan, &mut inspection);
    }
    let mut analyses = scan.analyses;
    let mut known_runs = scan.runs;
    known_runs.sort_by(|a, b| a.0.cmp(&b.0));
    analyses.sort_by(|a, b| a.run.cmp(&b.run));
    // Metadata-only transitive closure: preserving the direct original repairs
    // descendants through their owning ledger, while removal lists every record
    // that would otherwise be left with a missing ancestor.
    loop {
        let before = analyses.len();
        for (run, input) in &known_runs {
            if analyses.iter().any(|a| &a.run == run) {
                continue;
            }
            let mut dependent = false;
            if let Some(parents) = Some(&input.upstream) {
                for parent in parents {
                    let path = &parent.bundle;
                    match depends_on_inspected(&run.join(path), &analyses, &mut inspection) {
                        Ok(found) => dependent |= found,
                        Err(error) => inspection.issue(
                            InspectionCode::DependencyUnavailable,
                            &run.join(path),
                            error,
                        ),
                    }
                }
            }
            if dependent {
                analyses.push(AffectedAnalysis {
                    run: run.clone(),
                    result: inspection.name(run),
                    source_hash: String::new(),
                    can_preserve: true,
                    direct: false,
                    backing_analyses: vec![],
                });
            }
        }
        if before == analyses.len() {
            break;
        }
    }
    // Only explicit ownership collapses records into one analysis. Equal
    // names/hashes alone never merge distinct analyses.
    let mut owners = BTreeMap::new();
    for (run, input) in &known_runs {
        if let Some(owner) = input.analysis_run.as_deref() {
            let owner = lexical(&run.join(owner));
            if known_runs.iter().any(|(path, _)| lexical(path) == owner) {
                owners.insert(run.clone(), owner);
            } else {
                inspection.issue(
                    InspectionCode::DependencyUnavailable,
                    &owner,
                    "analysis owner is unavailable in inspected inventory",
                );
            }
        }
    }
    let mut groups: BTreeMap<PathBuf, Vec<AffectedAnalysis>> = BTreeMap::new();
    for analysis in analyses {
        let owner = owners
            .get(&analysis.run)
            .cloned()
            .unwrap_or_else(|| analysis.run.clone());
        groups.entry(owner).or_default().push(analysis);
    }
    // A logical analysis owns all its dedicated ledgers, including inputs other
    // than the overwritten file. Break/remove apply to that whole inventory;
    // preserve only touches the direct references marked above.
    for (run, _) in &known_runs {
        let owner = owners.get(run).unwrap_or(run);
        if let Some(backing) = groups.get_mut(owner)
            && !backing.iter().any(|analysis| &analysis.run == run)
        {
            backing.push(AffectedAnalysis {
                run: run.clone(),
                result: inspection.name(run),
                source_hash: String::new(),
                can_preserve: true,
                direct: false,
                backing_analyses: vec![],
            });
        }
    }
    let analyses: Vec<_> = groups
        .into_iter()
        .map(|(owner, mut backing)| {
            backing.sort_by_key(|a| a.run != owner);
            Ok(AffectedAnalysis {
                result: inspection.name(&owner),
                run: owner,
                source_hash: backing
                    .iter()
                    .find(|a| a.direct)
                    .map_or_else(String::new, |a| a.source_hash.clone()),
                can_preserve: backing.iter().all(|a| a.can_preserve),
                direct: backing.iter().any(|a| a.direct),
                backing_analyses: backing,
            })
        })
        .collect::<Result<_>>()?;
    let mut options = vec![
        SavePolicy::Cancel,
        SavePolicy::RemoveAnalysis,
        SavePolicy::BreakEvidence,
    ];
    if !analyses.is_empty() && analyses.iter().all(|a| a.can_preserve) {
        options.insert(1, SavePolicy::PreserveCopy);
    }
    let conflict = EvidenceConflict {
        target: target.into(),
        analyses,
        options,
    };
    inspection
        .issues
        .sort_by(|a, b| a.location.cmp(&b.location).then(a.reason.cmp(&b.reason)));
    inspection.issues.dedup();
    let inventory_revision = basecraft_io::hash_bytes(&serde_json::to_vec(&(
        stores,
        &conflict,
        &known_runs,
        &inspection.observations,
        &inspection.issues,
    ))?);
    Ok(EvidenceCheck {
        conflict,
        issues: inspection.issues,
        inventory_revision,
    })
}

// Save As companions are not store runs. Include each delegation location
// before reading it, so a removed owner can still be matched by its old path.
fn depends_on_inspected(
    bundle: &Path,
    analyses: &[AffectedAnalysis],
    inspection: &mut Inspection,
) -> Result<bool> {
    let mut location = lexical(bundle);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..32 {
        if analyses.iter().any(|a| lexical(&a.run) == location) {
            return Ok(true);
        }
        if !seen.insert(location.clone()) {
            return Err(invalid("input evidence contains a cycle"));
        }
        if !location.join("input.json").try_exists()? {
            return Err(invalid("ancestor input evidence is unavailable"));
        }
        let input = inspection.input(&location)?;
        let Some(origin) = &input.origin_run else {
            return Ok(false);
        };
        location = lexical(&location.join(origin));
    }
    Err(invalid("input evidence ancestry exceeds 32 references"))
}

/// Retain one owned input ledger before a temporary source disappears. Uses
/// the same checked, idempotent preserve-copy transition as a document save,
/// without touching other analyses that happen to reference the same file.
pub fn preserve_input(bundle: &Path) -> Result<()> {
    Mutation::begin()?.preserve_input(bundle)
}

fn preserve_input_locked(bundle: &Path) -> Result<()> {
    let input = read_input(bundle)?;
    let reference = input
        .source
        .as_ref()
        .ok_or_else(|| invalid("input has no source reference"))?;
    let target = bundle.join(&reference.path);
    resolve(
        &EvidenceConflict {
            target,
            analyses: vec![AffectedAnalysis {
                run: bundle.into(),
                result: "Retained operation input".into(),
                source_hash: reference.hash.clone(),
                can_preserve: true,
                direct: true,
                backing_analyses: vec![],
            }],
            options: vec![SavePolicy::PreserveCopy],
        },
        SavePolicy::PreserveCopy,
    )
}

/// Resolve a group under the caller's existing mutation guard. Two target
/// groups can contain the very same backing ledger. Removal of that exact
/// ledger is performed once; equal hashes/names never identify two ledgers.
/// The original groups remain intact in the Session receipt and action report.
pub(crate) fn resolve_group(
    conflict: &EvidenceConflict,
    policy: SavePolicy,
    removed: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<()> {
    if policy != SavePolicy::RemoveAnalysis {
        return resolve(conflict, policy);
    }
    let mut remaining = conflict.clone();
    remaining.analyses.retain_mut(|analysis| {
        if analysis.backing_analyses.is_empty() {
            return !removed.contains(&analysis.run);
        }
        analysis
            .backing_analyses
            .retain(|backing| !removed.contains(&backing.run));
        !analysis.backing_analyses.is_empty()
    });
    resolve(&remaining, policy)?;
    for analysis in remaining.analyses {
        if analysis.backing_analyses.is_empty() {
            removed.insert(analysis.run);
        } else {
            removed.extend(
                analysis
                    .backing_analyses
                    .into_iter()
                    .map(|backing| backing.run),
            );
        }
    }
    Ok(())
}

/// Apply explicit decisions before overwrite. A partial decision failure aborts
/// the save; earlier preserved copies/removed metadata remain recoverable.
pub fn resolve(conflict: &EvidenceConflict, policy: SavePolicy) -> Result<()> {
    let _locks = lock()?;
    if conflict.analyses.is_empty() {
        return Ok(());
    }
    if policy == SavePolicy::Cancel {
        return Err(crate::Error::EvidenceConflict(Box::new(conflict.clone())));
    }
    if !conflict.options.contains(&policy) {
        return Err(invalid(
            "old analysed bytes are unavailable; preserve-copy cannot repair this input",
        ));
    }
    for analysis in &conflict.analyses {
        let logical_owner = &analysis.run;
        let mut preserved: BTreeMap<String, PathBuf> = BTreeMap::new();
        let backing = if analysis.backing_analyses.is_empty() {
            std::slice::from_ref(analysis)
        } else {
            &analysis.backing_analyses
        };
        let mut backing: Vec<_> = backing.iter().collect();
        if policy == SavePolicy::PreserveCopy {
            // A leaf input ledger can own shared bytes without a cycle. The
            // logical run already depends on that ledger; delegate toward the
            // leaf, never from it back to its parent analysis.
            backing.sort_by_key(|analysis| (!analysis.direct, &analysis.run == logical_owner));
        }
        if policy == SavePolicy::RemoveAnalysis {
            // A parent workflow contains its child stores. Move the children
            // first, retaining their trash inside the recoverable parent tree.
            backing.sort_by_key(|analysis| std::cmp::Reverse(analysis.run.components().count()));
        }
        for affected in backing {
            let mut input = read_input(&affected.run)?;
            match policy {
                SavePolicy::PreserveCopy => {
                    if !affected.direct {
                        continue;
                    }
                    let reference = input
                        .source
                        .as_ref()
                        .ok_or_else(|| invalid("reference is missing"))?;
                    if reference.hash != affected.source_hash {
                        return Err(invalid("stale input preservation decision"));
                    }
                    let destination = affected.run.join("preserved-input");
                    // A public retry must never touch the live source or replace
                    // retained bytes. The owning ledger may have advanced already.
                    if input.state == basecraft_protocol::input_records::EvidenceState::Preserved {
                        let (owner, retained) = owning_ledger(&affected.run)?;
                        let effective = retained
                            .source
                            .as_ref()
                            .ok_or_else(|| invalid("reference is missing"))?;
                        verify_preserved(&owner.join("preserved-input"), effective)?;
                        preserved.insert(reference.hash.clone(), owner);
                        continue;
                    }
                    if input.state != basecraft_protocol::input_records::EvidenceState::Referenced {
                        return Err(invalid("input evidence changed; refresh the save decision"));
                    }
                    let source = affected.run.join(&reference.path);
                    if !same_source(&affected.run, reference, &conflict.target) {
                        return Err(invalid("input reference moved; refresh the save decision"));
                    }
                    if let Some(owner) = preserved.get(&reference.hash) {
                        verify_preserved(&owner.join("preserved-input"), reference)?;
                        input.origin_run = Some(relative(&affected.run, owner)?);
                        input.state = "preserved".into();
                        input
                            .extra
                            .insert("preserved_original_bytes".into(), serde_json::json!(0));
                        write_json(&affected.run.join("input.json"), &input)?;
                        continue;
                    }
                    let copied = if destination.try_exists()? {
                        // A previous ledger write may have failed after publication.
                        verify_preserved(&destination, reference)?;
                        0
                    } else {
                        let mut candidate = tempfile::NamedTempFile::new_in(&affected.run)?;
                        let mut hashing = crate::files::HashingWriter::new(candidate.as_file_mut());
                        let copied =
                            std::io::copy(&mut std::fs::File::open(&source)?, &mut hashing)?;
                        if hashing.hash() != reference.hash || copied != reference.stamp.size {
                            return Err(invalid(
                                "source changed: copied bytes are not the analysed original; overwrite cancelled",
                            ));
                        }
                        candidate.as_file().sync_all()?;
                        candidate
                            .persist_noclobber(&destination)
                            .map_err(|e| e.error)?;
                        basecraft_platform::files::sync_directory(&affected.run)?;
                        copied
                    };
                    preserved.insert(reference.hash.clone(), affected.run.clone());
                    input.state = "preserved".into();
                    input
                        .extra
                        .insert("preserved_original_bytes".into(), serde_json::json!(copied));
                    write_json(&affected.run.join("input.json"), &input)?;
                }
                SavePolicy::BreakEvidence => {
                    input.state = "broken".into();
                    write_json(&affected.run.join("input.json"), &input)?;
                }
                SavePolicy::RemoveAnalysis => {
                    let store = affected
                        .run
                        .parent()
                        .and_then(Path::parent)
                        .ok_or_else(|| invalid("run has no store"))?;
                    let trash = store.join("trash");
                    if !trash.exists() {
                        std::fs::create_dir(&trash)?;
                    }
                    if std::fs::symlink_metadata(&trash)?.file_type().is_symlink() {
                        return Err(invalid("unsafe analysis trash"));
                    }
                    let destination = trash.join(format!(
                        "{}-{}",
                        affected
                            .run
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
                        ulid::Ulid::generate()
                    ));
                    let catalogue = registry_lock()?;
                    index::changing(
                        &affected.run.join("input.json"),
                        Some(std::collections::BTreeSet::new()),
                    )?;
                    std::fs::rename(&affected.run, destination)?;
                    basecraft_platform::files::sync_directory(affected.run.parent().unwrap())?;
                    basecraft_platform::files::sync_directory(&trash)?;
                    basecraft_platform::files::sync_directory(store)?;
                    drop(catalogue);
                    index::register(store, None)?;
                }
                SavePolicy::Cancel => return Err(invalid("save cancelled")),
            }
        }
    }
    Ok(())
}

fn verify_preserved(path: &Path, reference: &InputReference) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != reference.stamp.size
        || basecraft_io::hash_file(path)? != reference.hash
    {
        return Err(invalid(
            "retained original is damaged; existing bytes preserved",
        ));
    }
    Ok(())
}

/// Follow only the bounded Save As delegation, never a source file or cache.
fn owning_ledger(bundle: &Path) -> Result<(PathBuf, SubmittedInput)> {
    let mut location = bundle.canonicalize()?;
    let mut input = read_input(&location)?;
    let mut visited = std::collections::HashSet::from([location.clone()]);
    for _ in 0..32 {
        let Some(origin) = &input.origin_run else {
            break;
        };
        location = location.join(origin).canonicalize()?;
        if !visited.insert(location.clone()) {
            return Err(invalid("input evidence contains a cycle"));
        }
        input = read_input(&location)?;
    }
    if input.origin_run.is_some() {
        return Err(invalid("input evidence ancestry exceeds 32 references"));
    }
    Ok((location, input))
}

/// Resolve evidence only on explicit verification/export/relink. No UI-event hashing.
pub fn verified_source(bundle: &Path, reference: &InputReference) -> Result<PathBuf> {
    let path = source_location(bundle, reference)?;
    if basecraft_io::hash_file(&path)? != reference.hash
        || std::fs::metadata(&path)?.len() != reference.stamp.size
    {
        return Err(invalid(
            "referenced input changed; original evidence unavailable (timestamp alone is not identity)",
        ));
    }
    Ok(path.canonicalize()?)
}

pub(crate) fn source_location(bundle: &Path, reference: &InputReference) -> Result<PathBuf> {
    let (mut location, mut input) = owning_ledger(bundle)?;
    if input.state == basecraft_protocol::input_records::EvidenceState::Broken {
        return Err(invalid("evidence chain explicitly broken"));
    }
    let primary_matches = input.source.as_ref().is_some_and(|source| {
        source.hash == reference.hash && source.stamp.size == reference.stamp.size
    });
    // Multi-input publications retain one ordinary ledger per original. Follow
    // only declared, bounded, hash-checked ancestry, with no filesystem scan.
    let mut matching = None;
    for (directory, publication) in ancestors(bundle)? {
        let declared = publication.inputs.iter().any(|artifact| {
            artifact.hash == reference.hash
                && artifact.size == reference.stamp.size
                && artifact.reference.as_ref().is_some_and(|source| {
                    source.hash == reference.hash && source.stamp.size == reference.stamp.size
                })
        });
        if !declared {
            continue;
        }
        let (owner, candidate) = owning_ledger(&directory)?;
        if candidate.source.as_ref().is_some_and(|source| {
            source.hash == reference.hash && source.stamp.size == reference.stamp.size
        }) {
            // Check every declared matching owner before selecting one: an
            // intact same-hash sibling must not hide an explicit evidence break.
            if candidate.state == basecraft_protocol::input_records::EvidenceState::Broken {
                return Err(invalid("evidence chain explicitly broken"));
            }
            if matching.is_none() {
                matching = Some((owner, candidate));
            }
        }
    }
    if !primary_matches && let Some((owner, candidate)) = matching {
        location = owner;
        input = candidate;
    }
    let effective = input
        .source
        .as_ref()
        .ok_or_else(|| invalid("reference evidence is missing"))?;
    if effective.hash != reference.hash {
        return Err(invalid("input reference identity changed"));
    }
    let path = if input.state == basecraft_protocol::input_records::EvidenceState::Preserved {
        location.join("preserved-input")
    } else {
        location.join(&effective.path)
    };
    Ok(path)
}

/// Honest repair: only a byte-verified original may replace a missing/changed reference.
pub fn relink(bundle: &Path, original: &Path) -> Result<()> {
    let locks = lock()?;
    locks.source(original)?;
    let local = read_input(bundle)?;
    let (owner, mut input) = owning_ledger(bundle)?;
    if local.source.as_ref().map(|r| &r.hash) != input.source.as_ref().map(|r| &r.hash) {
        return Err(invalid("delegated input reference identity changed"));
    }
    let reference = input
        .source
        .as_mut()
        .ok_or_else(|| invalid("generated input has no disk reference"))?;
    let original = original.canonicalize()?;
    if basecraft_io::hash_file(&original)? != reference.hash
        || std::fs::metadata(&original)?.len() != reference.stamp.size
    {
        return Err(invalid("selected file is not the analysed original"));
    }
    reference.path = relative(&owner, &original)?;
    reference.stamp = stamp(&original)?;
    let hash = reference.hash.clone();
    input.state = "referenced".into();
    write_json(&owner.join("input.json"), &input)?;
    // A registered run keeps one reference ledger per original beside the run.
    // Relink each ledger in this result's declared ancestry that names the same
    // bytes; an explicitly broken ledger stays broken.
    for (directory, _) in ancestors(bundle)? {
        let directory = directory.canonicalize()?;
        if directory == owner || !directory.join("input.json").is_file() {
            continue;
        }
        let mut ledger = read_input(&directory)?;
        if ledger.state == basecraft_protocol::input_records::EvidenceState::Broken
            || ledger.origin_run.is_some()
        {
            continue;
        }
        let Some(source) = ledger.source.as_mut() else {
            continue;
        };
        if source.hash != hash {
            continue;
        }
        source.path = relative(&directory, &original)?;
        source.stamp = stamp(&original)?;
        ledger.state = "referenced".into();
        write_json(&directory.join("input.json"), &ledger)?;
    }
    Ok(())
}

/// Explicit portable export materializes referenced dependencies; ordinary analysis does not.
pub fn package(
    result: &Path,
    destination: &Path,
    project: Option<&crate::project::Project>,
) -> Result<()> {
    let issues = crate::records::provenance::verify(result, project)?;
    if !issues.is_empty() {
        return Err(invalid(issues.join("; ")));
    }
    if destination.exists() {
        return Err(invalid("package destination already exists"));
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".basecraft-package-")
        .tempdir_in(parent)?;
    let bundle = crate::records::provenance::bundle_for_result(result, project);
    let mut publication = crate::records::provenance::inspect(result, project)?;
    if let Some(index) = publication
        .outputs
        .iter()
        .position(|a| Path::new(&a.logical_name).file_name() == result.file_name())
    {
        publication.outputs.swap(0, index);
    }
    let copied = staging.path().join(
        result
            .file_name()
            .ok_or_else(|| invalid("result has no filename"))?,
    );
    let target_bundle = crate::records::provenance::companion(&copied);
    std::fs::create_dir_all(target_bundle.join("artifacts"))?;
    // Rebase the whole immutable record graph, not only today's inputs.
    // Original record bytes are retained as checked artifacts; relocated records
    // get new identities and recomputed parent checksums in dependency order.
    let original_id = publication.id.clone();
    let upstream = ancestors(&bundle)?;
    let mut roots = vec![(bundle.clone(), publication.clone())];
    roots.extend(upstream);
    let species =
        crate::files::Format::from_path(result).ok() == Some(crate::files::Format::Delimitation);
    let sanger = if crate::sanger_review::is_review(result) {
        Some(result.to_path_buf())
    } else if publication.origin.starts_with("Sanger accepted consensus;") {
        publication
            .inputs
            .iter()
            .find(|a| a.logical_name == "review.sanger-review.json")
            .and_then(|a| a.retained.as_ref())
            .map(|name| bundle.join(name))
    } else {
        None
    };
    let mut species_paths = BTreeMap::new();
    if species || sanger.is_some() {
        for (directory, record) in &roots {
            for artifact in record.inputs.iter().chain(&record.outputs) {
                let original = if let Some(reference) = &artifact.reference {
                    let (owner, input) = owning_ledger(directory)?;
                    let original = input.source.as_ref().map_or_else(
                        || directory.join(&reference.path),
                        |source| owner.join(&source.path),
                    );
                    Some(original)
                } else {
                    artifact.retained.as_ref().map(|name| directory.join(name))
                };
                if let Some(original) = original {
                    species_paths.insert(normalized_path(&original), artifact.hash.clone());
                }
            }
        }
    }
    let (mut exported, archived) = export_history(&roots, &target_bundle)?;
    publication = exported.remove(0);
    if let Some(index) = publication
        .outputs
        .iter()
        .position(|a| Path::new(&a.logical_name).file_name() == result.file_name())
    {
        publication.outputs.swap(0, index);
    }
    for ancestor in exported {
        if !publication.parents.contains(&ancestor.id) {
            publication.parent_hashes.insert(
                ancestor.id.clone(),
                basecraft_io::hash_file(
                    &target_bundle
                        .join("records")
                        .join(format!("{}.json", ancestor.id)),
                )?,
            );
            publication.parents.push(ancestor.id);
        }
    }
    publication.inputs.extend(archived);
    for (index, artifact) in publication.outputs.iter_mut().enumerate() {
        let source = if index == 0 {
            result.to_path_buf()
        } else {
            result
                .parent()
                .unwrap_or(Path::new("."))
                .join(&artifact.logical_name)
        };
        let name = if index == 0 {
            copied
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else if source == crate::files::tree_sidecar(result) {
            crate::files::tree_sidecar(&copied)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            format!(
                "support-{}-{}",
                index,
                source.file_name().unwrap_or_default().to_string_lossy()
            )
        };
        copy(&source, &staging.path().join(&name))?;
        artifact.logical_name = name;
    }
    if species {
        let mut work = crate::files::load_delimitation(result)?;
        let final_folder = parent.canonicalize()?.join(
            destination
                .file_name()
                .ok_or_else(|| invalid("package destination has no name"))?,
        );
        let final_bundle = crate::records::provenance::companion(
            &final_folder.join(copied.file_name().unwrap_or_default()),
        );
        // Resolve declared paths to the checked, materialized dependency closure.
        // Immutable scientific reports stay byte-for-byte; only the derived
        // workspace metadata is relocated for use without its originals.
        crate::files::map_delimitation_paths(&mut work, |path| {
            species_paths
                .get(&normalized_path(Path::new(path)))
                .map_or_else(
                    || path.to_owned(),
                    |hash| {
                        final_bundle
                            .join("artifacts")
                            .join(hash.trim_start_matches("b3:"))
                            .display()
                            .to_string()
                    },
                )
        });
        work.exchange_fields
            .remove("basecraft_display_evidence_run");
        work.exchange_fields
            .remove("basecraft_alignment_evidence_run");
        for hypothesis in &mut work.hypotheses {
            if let Some(source) = hypothesis
                .provenance
                .parameters
                .get_mut("combined_source")
                .and_then(serde_json::Value::as_object_mut)
            {
                source.remove("evidence_run");
            }
        }
        let sidecar = crate::files::tree_sidecar(&copied);
        basecraft_io::write_atomic(&sidecar, |w| {
            crate::files::write_delimitation_sidecar_in(
                &work,
                &publication.outputs[0].hash,
                &final_folder,
                w,
            )
        })?;
        let side_hash = basecraft_io::hash_file(&sidecar)?;
        if let Some(artifact) = publication
            .outputs
            .iter_mut()
            .find(|a| a.logical_name == sidecar.file_name().unwrap_or_default().to_string_lossy())
        {
            artifact.hash = side_hash;
            artifact.size = std::fs::metadata(&sidecar)?.len();
            artifact.retained = None;
        } else {
            publication.outputs.push(ArtifactRevision {
                id: format!("{}:species-context", publication.id),
                logical_name: sidecar
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hash: side_hash,
                size: std::fs::metadata(&sidecar)?.len(),
                retained: None,
                reference: None,
            });
        }
    }
    if let Some(original) = &sanger {
        let review = if original == result {
            copied.clone()
        } else {
            copied.with_extension("sanger-review.json")
        };
        if original != result {
            copy(original, &review)?;
        }
        crate::sanger_review::relocate_package(original, &review, &target_bundle, &species_paths)?;
        let hash = basecraft_io::hash_file(&review)?;
        let size = std::fs::metadata(&review)?.len();
        if original == result {
            publication.outputs[0].hash = hash;
            publication.outputs[0].size = size;
            // Relocation changes the visible review, not its retained original artifact.
            publication.outputs[0].retained = None;
        } else {
            publication.outputs.push(ArtifactRevision {
                id: format!("{}:sanger-review", publication.id),
                logical_name: review
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hash,
                size,
                retained: None,
                reference: None,
            });
        }
    }
    // Complete tree exports materialize their exchange association, so
    // extraction still works with the original store absent.
    if let Ok(tree) = crate::files::load_tree(result)
        && crate::records::links::metadata_present(&crate::records::links::sidecar(result))?
    {
        let link = crate::records::links::AlignmentLink::read(result)?;
        let input_path = link.checked_snapshot(result, &tree.doc)?;
        let indexed = std::sync::Arc::new(basecraft_io::fasta::IndexedFasta::open(&input_path)?);
        let alignment = basecraft_docs::AlignmentDoc::from_fasta("Exported input", &indexed);
        let mut canonical = tempfile::Builder::new()
            .suffix(".afa")
            .tempfile_in(staging.path())?;
        crate::files::write_alignment(&alignment, canonical.as_file_mut())?;
        canonical.as_file().sync_all()?;
        let source_artifact = publication
            .inputs
            .first()
            .ok_or_else(|| invalid("tree export has no source"))?;
        let source_path = target_bundle.join(
            source_artifact
                .retained
                .as_ref()
                .ok_or_else(|| invalid("exported source is not retained"))?,
        );
        let paths = crate::records::links::AlignmentLink::create(
            &copied,
            &source_path,
            canonical.path(),
            link.alignment_version,
            link.inferred,
            None,
        )?;
        for path in paths {
            publication
                .outputs
                .push(basecraft_protocol::provenance::ArtifactRevision {
                    id: format!(
                        "{}:package-link:{}",
                        publication.id,
                        publication.outputs.len()
                    ),
                    logical_name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    hash: basecraft_io::hash_file(&path)?,
                    size: std::fs::metadata(&path)?.len(),
                    retained: None,
                    reference: None,
                });
        }
    }
    publication.origin = format!(
        "Portable export of revision {}. {}",
        original_id, publication.origin
    );
    crate::records::provenance::publish(&target_bundle, &publication)?;
    let authored = bundle.join("reports/methods.md");
    if authored.is_file() {
        copy(&authored, &target_bundle.join("reports/methods.md"))?;
    }
    let issues = crate::records::provenance::verify(&copied, None)?;
    if !issues.is_empty() {
        return Err(crate::Error::Damaged {
            path: copied.clone(),
            detail: format!(
                "export verification failed: {}; original source and records preserved",
                issues.join("; ")
            ),
        });
    }
    if destination.exists() {
        return Err(invalid("package destination appeared during export"));
    }
    // Publish a new directory without replacing a destination created concurrently.
    basecraft_platform::atomic::rename_noreplace(staging.path(), destination)?;
    Ok(())
}

fn normalized_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// Copy checked bytes into the package's content-addressed artifact directory.
fn materialize(
    directory: &Path,
    target: &Path,
    artifact: &mut basecraft_protocol::provenance::ArtifactRevision,
) -> Result<()> {
    let source = if let Some(reference) = &artifact.reference {
        verified_source(directory, reference)?
    } else {
        directory.join(
            artifact
                .retained
                .as_ref()
                .ok_or_else(|| invalid("input bytes unavailable"))?,
        )
    };
    let name = format!("artifacts/{}", artifact.hash.trim_start_matches("b3:"));
    let destination = target.join(&name);
    if !destination.try_exists()? {
        copy(&source, &destination)?;
    }
    if std::fs::metadata(&destination)?.len() != artifact.size
        || basecraft_io::hash_file(&destination)? != artifact.hash
    {
        return Err(invalid("evidence changed during export"));
    }
    artifact.retained = Some(name);
    artifact.reference = None;
    Ok(())
}

type RecordKey = (PathBuf, String);
type Publication = basecraft_protocol::provenance::Publication;
type ArtifactRevision = basecraft_protocol::provenance::ArtifactRevision;

/// Bounded, iterative postorder traversal of each bundle's parent graph. Export
/// records are explicit relocations, while original immutable JSON stays intact
/// as a hash-checked input artifact. No source records are rewritten.
fn export_history(
    roots: &[(PathBuf, Publication)],
    target: &Path,
) -> Result<(Vec<Publication>, Vec<ArtifactRevision>)> {
    let mut stack = Vec::new();
    let mut root_keys = Vec::new();
    for (directory, publication) in roots {
        let key = (directory.canonicalize()?, publication.id.clone());
        stack.push((key.clone(), false));
        root_keys.push(key);
    }
    let mut states = std::collections::HashMap::new();
    let mut records = std::collections::HashMap::new();
    let mut order = Vec::new();
    let mut record_bytes = 0usize;
    let mut scheduled = stack.len();
    while let Some((key, leaving)) = stack.pop() {
        if leaving {
            states.insert(key.clone(), 2);
            order.push(key);
            continue;
        }
        match states.get(&key) {
            Some(1) => return Err(invalid("export history contains a cycle")),
            Some(2) => continue,
            _ => {}
        }
        if key.1.is_empty()
            || !key
                .1
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(invalid("unsafe publication identity"));
        }
        let bytes = crate::records::provenance::read_record(
            &key.0.join("records").join(format!("{}.json", key.1)),
        )?;
        record_bytes = record_bytes.saturating_add(bytes.len());
        if record_bytes > 128 << 20 || scheduled > 10_000 {
            return Err(invalid("export history exceeds bounded record limit"));
        }
        let record: Publication = serde_json::from_slice(&bytes)?;
        if record.id != key.1 || record.schema_version != 1 {
            return Err(invalid("inconsistent export history record"));
        }
        states.insert(key.clone(), 1);
        stack.push((key.clone(), true));
        for parent in &record.parents {
            scheduled += 1;
            stack.push(((key.0.clone(), parent.clone()), false));
        }
        records.insert(key, (record, bytes));
    }
    let mut rebased = std::collections::HashMap::<RecordKey, Publication>::new();
    let mut hashes = std::collections::HashMap::<RecordKey, String>::new();
    let mut archives = Vec::new();
    let mut mapping = Vec::new();
    std::fs::create_dir_all(target.join("records"))?;
    for key in order {
        let (mut publication, bytes) = records
            .remove(&key)
            .ok_or_else(|| invalid("export history is missing"))?;
        let hash = basecraft_io::hash_bytes(&bytes);
        let name = format!("artifacts/{}", hash.trim_start_matches("b3:"));
        basecraft_io::write_atomic(&target.join(&name), |w| w.write_all(&bytes))?;
        let old_id = publication.id.clone();
        publication.id = ulid::Ulid::generate().to_string();
        mapping.push(ProvenanceExportMapping {
            original_id: old_id.clone(),
            original_hash: hash.clone(),
            export_id: publication.id.clone(),
            extra: BTreeMap::new(),
        });
        archives.push(ArtifactRevision {
            id: format!("{}:original-record", publication.id),
            logical_name: format!("original-record-{old_id}.json"),
            hash,
            size: bytes.len() as u64,
            retained: Some(name),
            reference: None,
        });
        let mut parents = Vec::new();
        let mut parent_hashes = BTreeMap::new();
        for parent in &publication.parents {
            let parent_key = (key.0.clone(), parent.clone());
            let rewritten = rebased
                .get(&parent_key)
                .ok_or_else(|| invalid("export parent missing"))?;
            parents.push(rewritten.id.clone());
            parent_hashes.insert(rewritten.id.clone(), hashes[&parent_key].clone());
        }
        publication.parents = parents;
        publication.parent_hashes = parent_hashes;
        for artifact in publication
            .inputs
            .iter_mut()
            .chain(&mut publication.outputs)
        {
            if artifact.retained.is_some() || artifact.reference.is_some() {
                materialize(&key.0, target, artifact)?;
            }
        }
        publication.origin = format!(
            "Relocated export record of {old_id}. {}",
            publication.origin
        );
        let path = target
            .join("records")
            .join(format!("{}.json", publication.id));
        write_json(&path, &publication)?;
        hashes.insert(key.clone(), basecraft_io::hash_file(&path)?);
        rebased.insert(key, publication);
    }
    let bytes = serde_json::to_vec_pretty(&serde_json::to_value(ProvenanceExportMap {
        schema_version: 1,
        records: mapping,
        extra: BTreeMap::new(),
    })?)?;
    let hash = basecraft_io::hash_bytes(&bytes);
    let name = format!("artifacts/{}", hash.trim_start_matches("b3:"));
    basecraft_io::write_atomic(&target.join(&name), |w| w.write_all(&bytes))?;
    archives.push(ArtifactRevision {
        id: format!("{}:export-map", ulid::Ulid::generate()),
        logical_name: "export-map.json".into(),
        hash,
        size: bytes.len() as u64,
        retained: Some(name),
        reference: None,
    });
    let exported = root_keys
        .iter()
        .map(|key| {
            rebased
                .get(key)
                .cloned()
                .ok_or_else(|| invalid("export root missing"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((exported, archives))
}

fn copy(source: &Path, destination: &Path) -> Result<()> {
    basecraft_io::write_atomic(destination, |w| {
        std::io::copy(&mut std::fs::File::open(source)?, w)?;
        Ok(())
    })?;
    Ok(())
}

/// Walk only declared immutable ancestor records. No ancestor input is copied.
/// The mutable evidence ledger stays in its owning run so later preservation or
/// explicit evidence-break decisions remain visible to descendants.
pub(crate) fn ancestors(
    bundle: &Path,
) -> Result<Vec<(PathBuf, basecraft_protocol::provenance::Publication)>> {
    let mut stack = vec![(bundle.to_path_buf(), false)];
    let mut states = std::collections::HashMap::new();
    let mut seen_records = std::collections::HashSet::new();
    let mut records = Vec::new();
    let mut bytes_read = 0usize;
    while let Some((location, leaving)) = stack.pop() {
        let location = location.canonicalize()?;
        if leaving {
            states.insert(location, 2);
            continue;
        }
        match states.get(&location) {
            Some(1) => return Err(invalid("evidence ancestry contains a cycle")),
            Some(2) => continue,
            _ => {}
        }
        states.insert(location.clone(), 1);
        stack.push((location.clone(), true));
        if !location.join("input.json").is_file() {
            continue;
        }
        let input = read_input(&location)?;
        if let Some(origin) = &input.origin_run {
            stack.push((location.join(origin), false));
        }
        if let Some(upstream) = Some(&input.upstream) {
            for reference in upstream {
                if records.len() + stack.len() >= 10_000 {
                    return Err(invalid("evidence ancestry exceeds 10,000 references"));
                }
                let directory = location.join(&reference.bundle).canonicalize()?;
                let id = &reference.id;
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    return Err(invalid("unsafe ancestor identity"));
                }
                stack.push((directory.clone(), false));
                if !seen_records.insert((directory.clone(), id.to_owned())) {
                    continue;
                }
                let bytes = crate::records::provenance::read_record(
                    &directory.join("records").join(format!("{id}.json")),
                )?;
                bytes_read = bytes_read.saturating_add(bytes.len());
                if bytes_read > 128 << 20 {
                    return Err(invalid("evidence ancestry exceeds 128 MiB"));
                }
                if reference.hash != basecraft_io::hash_bytes(&bytes) {
                    return Err(invalid("ancestor record hash differs"));
                }
                let publication: basecraft_protocol::provenance::Publication =
                    serde_json::from_slice(&bytes)?;
                if &publication.id != id || publication.schema_version != 1 {
                    return Err(invalid("unsupported or inconsistent ancestor record"));
                }
                records.push((directory, publication));
            }
        }
    }
    Ok(records)
}

/// Verify the immutable upstream records using the shared bounded ancestry walk.
pub fn verify_ancestors(bundle: &Path) -> Result<Vec<String>> {
    let mut issues = Vec::new();
    for (directory, publication) in ancestors(bundle)? {
        issues.extend(crate::records::provenance::verify_records(
            &directory,
            &publication,
        )?);
    }
    Ok(issues)
}

/// Link generated bytes and existing normal input ledgers without inventing an
/// original reference. The caller retains the payload as a publication artifact.
pub fn link_generated_input(
    run: &ArtifactRun,
    dependencies: &[PathBuf],
    payload: &[u8],
    transformation: &str,
    result_label: &str,
) -> Result<()> {
    Mutation::begin()?.link_generated_input(
        run,
        dependencies,
        payload,
        transformation,
        result_label,
    )
}

/// Same publication while the caller already holds the evidence lock through a save.
pub(crate) fn link_generated_input_locked(
    run: &ArtifactRun,
    dependencies: &[PathBuf],
    payload: &[u8],
    transformation: &str,
    result_label: &str,
    register: impl FnOnce(&ArtifactRun) -> Result<()>,
) -> Result<()> {
    let mut upstream = Vec::new();
    for dependency in dependencies {
        let bytes = crate::records::provenance::read_record(&dependency.join("manifest.json"))?;
        let publication: basecraft_protocol::provenance::Publication =
            serde_json::from_slice(&bytes)?;
        upstream.push(basecraft_protocol::input_records::UpstreamLink {
            bundle: (crate::records::input_evidence::relative(run.directory(), dependency)?).into(),
            id: publication.id.clone(),
            hash: basecraft_io::hash_bytes(&bytes),
        });
    }
    crate::records::input_evidence::write_json(
        &run.artifact("input.json")?,
        &crate::records::input_evidence::SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: basecraft_io::hash_bytes(payload),
            submitted_size: payload.len() as u64,
            transformation: transformation.into(),
            state: "generated".into(),
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
    )?;
    register(run)
}

#[cfg(test)]
mod store_scan_tests {
    use super::*;

    fn fixture(root: &Path, source: &Path) -> ArtifactRun {
        let run = ArtifactRun::create(&root.join("species.spart"), "scan-test").unwrap();
        write_json(
            &run.artifact("input.json").unwrap(),
            &SubmittedInput {
                schema_version: 1,
                source: Some(InputReference {
                    path: relative(run.directory(), source).unwrap(),
                    stamp: stamp(source).unwrap(),
                    hash: basecraft_io::hash_file(source).unwrap(),
                    extra: BTreeMap::new(),
                }),
                submitted_hash: basecraft_io::hash_file(source).unwrap(),
                submitted_size: stamp(source).unwrap().size,
                transformation: "identity-v1".into(),
                state: "referenced".into(),
                analysis_run: None,
                proposal_target: None,
                upstream: Vec::new(),
                origin_run: None,
                extra: BTreeMap::new(),
            },
        )
        .unwrap();
        write_json(
            &run.artifact("run.json").unwrap(),
            &serde_json::json!({"result":"species.spart"}),
        )
        .unwrap();
        run
    }

    #[test]
    fn disappearing_store_keeps_prior_evidence_and_marks_uncertainty() {
        for delete in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("input.nwk");
            std::fs::write(&source, "(a:1,b:1);").unwrap();
            let run = fixture(root.path(), &source);
            let store = run.directory().parent().unwrap().parent().unwrap();
            let mut scan = StoreScan::default();
            let mut inspection = Inspection::default();
            scan_store(store, &source, &mut scan, &mut inspection);
            assert_eq!(scan.analyses.len(), 1);
            if delete {
                std::fs::remove_dir_all(store).unwrap();
            } else {
                std::fs::rename(store, root.path().join("moved-store")).unwrap();
            }
            // Controlled transition between two actual inspection boundaries.
            // Never discard successfully inspected evidence on a later failure.
            scan_store(store, &source, &mut scan, &mut inspection);
            assert_eq!(scan.analyses.len(), 1);
            assert_eq!(inspection.issues[0].code, InspectionCode::StoreUnavailable);
            let check = check_in(&source, &[store.into()]).unwrap();
            assert!(!check.is_complete());
            assert!(conflicts_in(&source, &[store.into()]).is_err());
        }
    }

    #[test]
    fn missing_store_does_not_discard_other_store_conflicts() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.nwk");
        std::fs::write(&source, "(a:1,b:1);").unwrap();
        let run = fixture(root.path(), &source);
        let store = run.directory().parent().unwrap().parent().unwrap();
        let check = check_in(&source, &[root.path().join("missing"), store.into()]).unwrap();
        assert!(!check.is_complete());
        assert_eq!(check.conflict.analyses.len(), 1);
        assert_eq!(check.conflict.analyses[0].run, run.directory());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_run_symlinks_are_not_treated_as_disappeared_directories() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.nwk");
        std::fs::write(&source, "(a:1,b:1);").unwrap();
        let run = fixture(root.path(), &source);
        let runs = run.directory().parent().unwrap();
        let store = runs.parent().unwrap();
        std::os::unix::fs::symlink(root.path().join("missing"), runs.join("unsafe")).unwrap();
        assert!(conflicts_in(&source, &[store.into()]).is_err());
    }

    #[test]
    fn present_store_with_damaged_metadata_still_refuses_save() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.nwk");
        std::fs::write(&source, "(a:1,b:1);").unwrap();
        let run = fixture(root.path(), &source);
        let store = run.directory().parent().unwrap().parent().unwrap();
        // A registered reference cannot silently lose its run details.
        std::fs::remove_file(run.artifact("run.json").unwrap()).unwrap();
        assert!(matches!(
            conflicts_in(&source, &[store.into()]),
            Err(crate::Error::EvidenceCheckIncomplete(_))
        ));
        std::fs::write(run.artifact("input.json").unwrap(), "damaged").unwrap();
        assert!(conflicts_in(&source, &[store.into()]).is_err());
        std::fs::remove_file(store.join("store.json")).unwrap();
        assert!(conflicts_in(&source, &[store.into()]).is_err());
        assert!(store.is_dir());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "(a:1,b:1);");
    }
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

#[cfg(test)]
#[path = "input_evidence/export_record_format_tests.rs"]
mod export_record_format_tests;
