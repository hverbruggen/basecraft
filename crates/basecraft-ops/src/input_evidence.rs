//! Referenced-input lifecycle. Only known stores are inspected, never filesystem trees.
use crate::links::AlignmentLinkExt;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use basecraft_protocol::provenance::{FileStamp, InputReference};
use basecraft_protocol::records::StoreRegistry;
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

use crate::{Result, analysis_store::ArtifactRun, error::invalid};

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
    lock: EvidenceLock,
}

enum EvidenceLock {
    Legacy { _file: std::fs::File },
    Checked(Box<crate::service_reads::ServiceLockGuard>),
}

impl Mutation {
    pub fn begin() -> Result<Self> {
        Ok(Self {
            lock: EvidenceLock::Legacy { _file: lock()? },
        })
    }

    #[allow(dead_code)]
    pub(crate) fn begin_with_access(
        context: std::sync::Arc<crate::service_reads::ServiceContext>,
        access: &dyn crate::access::Access,
    ) -> Result<Self> {
        crate::service_reads::acquire_lock(context, access, None).map(|guard| Self {
            lock: EvidenceLock::Checked(Box::new(guard)),
        })
    }
    /// Checks the retained guard only; never reacquires or authorizes legacy
    /// write/register methods. Those remain outside confined activation.
    #[allow(dead_code)]
    pub fn reauthorize(&self, access: &dyn crate::access::Access) -> Result<()> {
        match &self.lock {
            EvidenceLock::Checked(guard) => guard.reauthorize(access),
            EvidenceLock::Legacy { .. } => Err(crate::access::AccessError::Denied(
                crate::access::Denial::ServiceUnavailable,
            )
            .into()),
        }
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
        reference_input_locked(anchor, source, hash, dependencies, origin)
    }
}

fn reference_input_locked(
    anchor: &Path,
    source: &Path,
    hash: &str,
    dependencies: &[PathBuf],
    origin: &str,
) -> Result<ArtifactRun> {
    use basecraft_protocol::provenance::{
        ArtifactRevision, Completeness, InputReference, Publication,
    };
    let run =
        crate::analysis_store::ArtifactRun::create(anchor, &ulid::Ulid::generate().to_string())?;
    let source = source.canonicalize()?;
    if basecraft_io::hash_file(&source)? != hash {
        return Err(invalid("Input changed while registering its evidence"));
    }
    let reference = InputReference {
        path: crate::input_evidence::relative(run.directory(), &source)?,
        stamp: crate::input_evidence::stamp(&source)?,
        hash: hash.into(),
        extra: BTreeMap::new(),
    };
    let mut input = crate::input_evidence::SubmittedInput {
        schema_version: 1,
        source: Some(reference.clone()),
        submitted_hash: hash.into(),
        submitted_size: reference.stamp.size,
        transformation: "identity-v1".into(),
        state: "referenced".into(),
        extra: BTreeMap::new(),
    };
    let source_bundle = crate::provenance::bundle_for_result(&source);
    let mut upstream = Vec::new();
    for dependency in dependencies.iter().chain(std::iter::once(&source_bundle)) {
        let manifest = dependency.join("manifest.json");
        if manifest.is_file() {
            let bytes = crate::provenance::read_record(&manifest)?;
            let record: Publication = serde_json::from_slice(&bytes)?;
            upstream.push(serde_json::json!({"bundle":crate::input_evidence::relative(run.directory(), dependency)?,"id":record.id,"hash":basecraft_io::hash_bytes(&bytes)}));
        }
    }
    if !upstream.is_empty() {
        input
            .extra
            .insert("upstream".into(), serde_json::json!(upstream));
    }
    crate::input_evidence::write_json(&run.artifact("input.json")?, &input)?;
    crate::input_evidence::write_json(
        &run.artifact("run.json")?,
        &serde_json::to_value(basecraft_protocol::records::AnalysisRun {
            schema_version: 1,
            id: run.id().into(),
            result: anchor
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_owned),
            status: basecraft_protocol::records::AnalysisRunStatus::Referenced,
            outputs: None,
            extra: BTreeMap::new(),
        })?,
    )?;
    crate::input_evidence::register(&run)?;
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
    crate::provenance::publish(run.directory(), &publication)?;
    Ok(run)
}

pub use basecraft_protocol::input_records::SubmittedInput;

/// `input.json` field marking a referenced source that is also the document
/// a sequence-proposal run applied its proposals to. Saving that document is
/// the expected next step, so the source does not guard saves; the record and
/// its hashes stay as written.
pub const PROPOSAL_TARGET: &str = "proposal_target";

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
    /// Complete backing decisions for this logical analysis. Empty for legacy
    /// single-ledger records. Each child retains its own hash/direct guard.
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
        let result = crate::provenance::read_record_with_budget(path, &mut limit);
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
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid("analysis input must be an ordinary file"));
    }
    let time = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| invalid("input timestamp is before Unix epoch"))?;
    #[cfg(unix)]
    let (device, inode) = {
        use std::os::unix::fs::MetadataExt;
        (Some(metadata.dev()), Some(metadata.ino()))
    };
    #[cfg(windows)]
    let (device, inode) = {
        let (device, inode) = basecraft_platform::files::identity(path)?;
        (Some(device), Some(inode))
    };
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
    let bytes = crate::provenance::read_record(&run.join("input.json"))?;
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
        || record.state == "generated" && record.source.is_some()
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
    basecraft_io::write_atomic(path, |w| w.write_all(&bytes))?;
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

/// Shared lock covers registration and the entire guarded replacement, including
/// copy/decision publication. It does not claim to lock external editors or sync.
pub(crate) fn evidence_lock_path(state: &Path) -> PathBuf {
    state.join("analysis-evidence.lock")
}

pub fn lock() -> Result<std::fs::File> {
    let registry = registry()?;
    let parent = registry
        .parent()
        .ok_or_else(|| invalid("registry has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(evidence_lock_path(parent))?;
    lock.lock_exclusive()
        .map_err(|_| invalid("analysis evidence is busy; retry shortly"))?;
    Ok(lock)
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
    match std::fs::File::open(registry()?) {
        Ok(file) => {
            let length = file.metadata()?.len();
            let bytes = crate::provenance::read_record_from_open(
                file,
                length,
                &mut (1 << 20),
                1 << 20,
                crate::provenance::RecordLimit::Registry,
            )?;
            parse_store_registry(&bytes)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(empty_registry()),
        Err(e) => Err(e.into()),
    }
}

// Unwired contextual consumers. Keep the original missing-component error until
// the selected consumer observes or explicitly projects the checked absence.
#[allow(dead_code)]
mod service_registry {
    use super::{
        Inspection, PathBuf, Result, StoreRegistry, empty_registry, parse_store_registry,
        validate_store_paths,
    };
    use crate::{access::Access, provenance::RecordLimit, service_reads::ServiceReadContext};
    use basecraft_platform::files::CheckedRead;

    enum RegistryBytes {
        Present(Vec<u8>),
        Absent { missing: std::io::Error },
    }
    struct RegistryByteRead {
        location: PathBuf,
        outcome: Result<RegistryBytes>,
    }

    fn registry_bytes_with_access(
        context: &ServiceReadContext,
        access: &dyn Access,
        remaining: &mut u64,
        label: RecordLimit,
    ) -> RegistryByteRead {
        let outcome = context
            .registry_input(access)
            .and_then(|input| match input {
                CheckedRead::File(file) => {
                    let length = file.metadata()?.len();
                    let result = crate::provenance::read_record_from_open(
                        file,
                        length,
                        remaining,
                        1 << 20,
                        label,
                    );
                    result.map(RegistryBytes::Present)
                }
                CheckedRead::Absent(absence) => {
                    let (_, missing) = absence.into_parts();
                    Ok(RegistryBytes::Absent { missing })
                }
            });
        RegistryByteRead {
            location: context.registry_location().into(),
            outcome,
        }
    }

    fn store_registry_with_access(
        context: &ServiceReadContext,
        access: &dyn Access,
    ) -> Result<StoreRegistry> {
        let read =
            registry_bytes_with_access(context, access, &mut (1 << 20), RecordLimit::Registry);
        let value = match read.outcome? {
            RegistryBytes::Present(bytes) => parse_store_registry(&bytes)?,
            RegistryBytes::Absent { .. } => empty_registry(),
        };
        validate_store_paths(&value.stores)?;
        Ok(value)
    }

    impl Inspection {
        fn registry_with_access(
            &mut self,
            context: &ServiceReadContext,
            access: &dyn Access,
            remaining: &mut u64,
        ) -> Result<Vec<PathBuf>> {
            self.registry_read(registry_bytes_with_access(
                context,
                access,
                remaining,
                RecordLimit::Provenance,
            ))
        }

        fn registry_read(&mut self, read: RegistryByteRead) -> Result<Vec<PathBuf>> {
            // Hard access/namespace failures must never look like inspected data.
            if matches!(
                &read.outcome,
                Err(crate::Error::Access(
                    crate::access::AccessError::Denied(_)
                        | crate::access::AccessError::DestinationExists
                ))
            ) {
                return read.outcome.map(|_| Vec::new());
            }
            let result = match read.outcome {
                Ok(RegistryBytes::Present(bytes)) => Ok(bytes),
                Ok(RegistryBytes::Absent { missing }) => {
                    let error = crate::Error::Io(missing);
                    self.observe(&read.location, &Err(error));
                    return Ok(Vec::new());
                }
                Err(error) => Err(error),
            };
            self.observe(&read.location, &result);
            // Unlike checked absence, a content/metadata NotFound is an ordinary
            // failed read and must produce an issue, never empty success.
            Ok(self.parse_registry(&read.location, result))
        }
    }
    #[cfg(all(test, target_os = "linux"))]
    mod tests {
        use super::*;
        use crate::service_reads::tests::{Bound, context};
        use std::io::{self, Read};
        use std::path::Path;

        #[test]
        fn checked_absence_keeps_logical_leaf_original_error_and_budget() {
            for existing_parent in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let context = context(temp.path());
                let path = context.registry_location();
                if existing_parent {
                    std::fs::create_dir(path.parent().unwrap()).unwrap();
                }
                let mut budget = 123;
                let read = registry_bytes_with_access(
                    &context,
                    &Bound(&context),
                    &mut budget,
                    RecordLimit::Registry,
                );
                assert_eq!(budget, 123);
                assert_eq!(read.location, path);
                let Ok(RegistryBytes::Absent { ref missing }) = read.outcome else {
                    panic!()
                };
                assert_eq!(missing.kind(), io::ErrorKind::NotFound);
                assert_eq!(missing.raw_os_error(), Some(2));
                let expected = format!("unavailable: {missing}");
                let mut inspection = Inspection::default();
                assert_eq!(inspection.registry_read(read).unwrap().len(), 0);
                assert_eq!(inspection.observations.len(), 1);
                assert_eq!(inspection.observations[path], expected);
                assert_eq!(inspection.issues.len(), 0);
                assert_eq!(
                    store_registry_with_access(&context, &Bound(&context))
                        .unwrap()
                        .stores
                        .len(),
                    0
                );
                assert_eq!(path.parent().unwrap().exists(), existing_parent);
                assert!(!path.exists());
            }
        }

        #[test]
        fn default_and_structural_denials_create_no_observation_or_default() {
            let temp = tempfile::tempdir().unwrap();
            let context = context(temp.path());
            let mut inspection = Inspection::default();
            let mut budget = 999;
            assert!(
                inspection
                    .registry_with_access(&context, &crate::access::TrustedLegacy, &mut budget)
                    .is_err()
            );
            assert_eq!(budget, 999);
            assert!(inspection.observations.is_empty());
            assert_eq!(inspection.issues.len(), 0);
            std::fs::create_dir(temp.path().join("state")).unwrap();
            std::os::unix::fs::symlink(temp.path().join("missing"), context.registry_location())
                .unwrap();
            assert!(
                inspection
                    .registry_with_access(&context, &Bound(&context), &mut budget)
                    .is_err()
            );
            assert_eq!(budget, 999);
            assert!(inspection.observations.is_empty());
            assert_eq!(inspection.issues.len(), 0);
        }

        #[test]
        fn present_unknown_fields_limits_and_distinct_parser_contracts() {
            let temp = tempfile::tempdir().unwrap();
            let context = context(temp.path());
            let path = context.registry_location();
            std::fs::create_dir(path.parent().unwrap()).unwrap();
            let outside = temp.path().join("outside-not-created");
            let value = serde_json::json!({"schema_version": 1, "stores": [outside], "future": {"keep":true}});
            let bytes = serde_json::to_vec(&value).unwrap();
            std::fs::write(path, &bytes).unwrap();
            let store = store_registry_with_access(&context, &Bound(&context)).unwrap();
            assert_eq!(store.extra["future"], value["future"]);
            let mut inspection = Inspection::default();
            let mut budget = 1 << 20;
            assert_eq!(
                inspection
                    .registry_with_access(&context, &Bound(&context), &mut budget)
                    .unwrap(),
                std::slice::from_ref(&outside)
            );
            assert_eq!(budget, (1 << 20) - bytes.len() as u64);
            assert_eq!(
                inspection.observations[path],
                basecraft_io::hash_bytes(&bytes)
            );
            assert!(!outside.exists());
            // Inspection retains valid entries and issues for invalid entries;
            // the ordinary typed parser rejects the entire mixed inventory.
            std::fs::write(
                path,
                serde_json::to_vec(
                    &serde_json::json!({"schema_version":1,"stores":[outside,17,"relative"]}),
                )
                .unwrap(),
            )
            .unwrap();
            assert!(store_registry_with_access(&context, &Bound(&context)).is_err());
            let mut inspection = Inspection::default();
            assert_eq!(
                inspection
                    .registry_with_access(&context, &Bound(&context), &mut (1 << 20))
                    .unwrap(),
                std::slice::from_ref(&outside)
            );
            assert_eq!(inspection.issues.len(), 2);
            for count in [4096, 4097] {
                std::fs::write(path, serde_json::to_vec(&serde_json::json!({"schema_version":1,"stores": vec![outside.clone(); count]})).unwrap()).unwrap();
                assert_eq!(
                    store_registry_with_access(&context, &Bound(&context)).is_ok(),
                    count == 4096
                );
                let mut inspection = Inspection::default();
                assert_eq!(
                    inspection
                        .registry_with_access(&context, &Bound(&context), &mut (1 << 20))
                        .unwrap()
                        .len(),
                    4096
                );
                assert_eq!(inspection.issues.len(), usize::from(count > 4096));
            }
            std::fs::write(path, vec![b' '; (1 << 20) + 1]).unwrap();
            assert!(
                matches!(store_registry_with_access(&context, &Bound(&context)), Err(crate::Error::Invalid(s)) if s == "known-store registry exceeds 1 MiB")
            );
            let mut inspection = Inspection::default();
            let mut budget = 1 << 22;
            inspection
                .registry_with_access(&context, &Bound(&context), &mut budget)
                .unwrap();
            assert_eq!(budget, 1 << 22); // metadata rejection precedes content
            assert!(
                inspection.observations[path].contains("provenance record exceeds reading limit")
            );
        }

        #[test]
        fn ordinary_permission_failure_is_not_absence() {
            use std::os::unix::fs::PermissionsExt;
            let temp = tempfile::tempdir().unwrap();
            let context = context(temp.path());
            let path = context.registry_location();
            std::fs::create_dir(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"{}").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o0)).unwrap();
            let read = registry_bytes_with_access(
                &context,
                &Bound(&context),
                &mut 100,
                RecordLimit::Registry,
            );
            assert!(
                matches!(&read.outcome, Err(crate::Error::Access(crate::access::AccessError::Io(e))) if e.kind() == io::ErrorKind::PermissionDenied && e.raw_os_error() == Some(13))
            );
            let mut inspection = Inspection::default();
            inspection.registry_read(read).unwrap();
            assert_eq!(inspection.issues.len(), 1);
            assert_eq!(inspection.observations.len(), 1);
            assert!(store_registry_with_access(&context, &Bound(&context)).is_err());
        }

        #[test]
        fn validated_denial_has_no_observation_or_default() {
            struct Revoke<'a>(&'a ServiceReadContext);
            impl Access for Revoke<'_> {
                fn boundary(&self) -> Option<&Path> {
                    None
                }
                fn resource(
                    &self,
                    _: &Path,
                    _: crate::access::ResourceAction,
                ) -> crate::access::Result<PathBuf> {
                    panic!("resource")
                }
                fn service_read(
                    &self,
                    request: &crate::service_reads::ServiceReadRequest<'_>,
                    phase: crate::service_reads::ServiceReadPhase<'_>,
                ) -> crate::access::Result<()> {
                    use crate::service_reads::ServiceReadPhase;
                    assert!(request.permits(self.0, &ServiceReadPhase::Eligibility));
                    if matches!(phase, ServiceReadPhase::Validated(_)) {
                        Err(crate::access::AccessError::Denied(
                            crate::access::Denial::ServiceUnavailable,
                        ))
                    } else {
                        Ok(())
                    }
                }
            }
            let temp = tempfile::tempdir().unwrap();
            let context = context(temp.path());
            let mut inspection = Inspection::default();
            let mut budget = 100;
            assert!(
                inspection
                    .registry_with_access(&context, &Revoke(&context), &mut budget)
                    .is_err()
            );
            assert_eq!(budget, 100);
            assert!(inspection.observations.is_empty());
            assert_eq!(inspection.issues.len(), 0);
            assert!(store_registry_with_access(&context, &Revoke(&context)).is_err());
        }

        #[test]
        fn observation_seam_consumes_original_absence_source() {
            let source = std::sync::Arc::new(());
            let missing = io::Error::new(io::ErrorKind::NotFound, Source(source.clone()));
            let path = PathBuf::from("logical-leaf");
            // Injection only at the actual observation seam. Production Absent
            // construction requires platform checked absence above.
            let read = RegistryByteRead {
                location: path.clone(),
                outcome: Ok(RegistryBytes::Absent { missing }),
            };
            let Ok(RegistryBytes::Absent { ref missing }) = read.outcome else {
                panic!()
            };
            assert!(std::sync::Arc::ptr_eq(
                &source,
                &missing
                    .get_ref()
                    .unwrap()
                    .downcast_ref::<Source>()
                    .unwrap()
                    .0
            ));
            let mut inspection = Inspection::default();
            assert_eq!(inspection.registry_read(read).unwrap().len(), 0);
            assert_eq!(
                inspection.observations[&path],
                "unavailable: retained custom source"
            );
            assert_eq!(std::sync::Arc::strong_count(&source), 1);
            assert_eq!(inspection.issues.len(), 0);
        }

        #[derive(Debug)]
        struct Source(std::sync::Arc<()>);
        impl std::fmt::Display for Source {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("retained custom source")
            }
        }
        impl std::error::Error for Source {}
        #[test]
        fn partial_failure_charges_bytes_and_retains_source_without_success_hash() {
            struct Failing {
                first: bool,
                error: Option<io::Error>,
            }
            impl Read for Failing {
                fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                    if self.first {
                        self.first = false;
                        bytes[..3].copy_from_slice(b"abc");
                        Ok(3)
                    } else {
                        Err(self.error.take().unwrap())
                    }
                }
            }
            let source = std::sync::Arc::new(());
            let mut budget = 100;
            let outcome = crate::provenance::read_record_from_open(
                Failing {
                    first: true,
                    error: Some(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        Source(source.clone()),
                    )),
                },
                0,
                &mut budget,
                1 << 20,
                RecordLimit::Registry,
            )
            .map(RegistryBytes::Present);
            assert_eq!(budget, 97);
            let Err(crate::Error::Io(error)) = &outcome else {
                panic!()
            };
            assert!(std::sync::Arc::ptr_eq(
                &source,
                &error.get_ref().unwrap().downcast_ref::<Source>().unwrap().0
            ));
            let path = PathBuf::from("logical-registry");
            let mut inspection = Inspection::default();
            inspection
                .registry_read(RegistryByteRead {
                    location: path.clone(),
                    outcome,
                })
                .unwrap();
            assert_eq!(
                inspection.observations[&path],
                "unavailable: retained custom source"
            );
            assert_eq!(inspection.issues.len(), 1);
            assert_eq!(std::sync::Arc::strong_count(&source), 1);
        }
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
    let mut all = stores()?;
    if !all.iter().any(|p| p == store) {
        if all.len() >= 4096 {
            return Err(invalid("known-store registry limit reached"));
        }
        all.push(store.into());
        let mut value = store_registry()?;
        value.stores = all;
        // The legacy Value builder sorted nested extension keys as well.
        write_json(&registry()?, &serde_json::to_value(value)?)?;
    }
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
    let mut inspection = Inspection::default();
    let path = registry()?;
    let result = inspection.record_bounded(&path, 1 << 20);
    let mut all = match result {
        Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        result => inspection.parse_registry(&path, result),
    };
    let target = std::path::absolute(target)?;
    let canonical = target.canonicalize().unwrap_or_else(|_| target.clone());
    for parent in canonical.ancestors().skip(1) {
        let nearby = parent.join(".basecraft/analyses-v1");
        match nearby.try_exists() {
            Ok(true) if !all.contains(&nearby) => all.push(nearby),
            Err(error) => inspection.issue(InspectionCode::StoreUnavailable, &nearby, error),
            _ => {}
        }
    }
    inspect_in(&canonical, &all, inspection)
}

impl Inspection {
    fn parse_registry(&mut self, path: &Path, result: Result<Vec<u8>>) -> Vec<PathBuf> {
        let mut all = Vec::new();
        match result {
            Ok(bytes) => {
                let parsed = serde_json::from_slice::<serde_json::Value>(&bytes);
                match parsed {
                    Ok(value) if value["schema_version"] == 1 && value["stores"].is_array() => {
                        if let Some(paths) = value["stores"].as_array() {
                            if paths.len() > 4096 {
                                self.issue(
                                    InspectionCode::LimitExceeded,
                                    path,
                                    "known-store registry exceeds 4096 stores",
                                );
                            }
                            for (index, entry) in paths.iter().take(4096).enumerate() {
                                match entry.as_str().map(PathBuf::from) {
                                    Some(store) if store.is_absolute() => all.push(store),
                                    _ => self.issue(
                                        InspectionCode::RegistryUnavailable,
                                        path,
                                        format!(
                                            "known-store entry {index} must be an absolute path"
                                        ),
                                    ),
                                }
                            }
                        }
                    }
                    Ok(_) => self.issue(
                        InspectionCode::RegistryUnavailable,
                        path,
                        "unsupported known-store registry",
                    ),
                    Err(error) => self.issue(InspectionCode::RegistryUnavailable, path, error),
                }
            }
            Err(error) => self.issue(InspectionCode::RegistryUnavailable, path, error),
        }
        all
    }
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
    if !entry.file_type()?.is_dir() {
        return Err(invalid("unsafe entry in analysis store"));
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
    for field in ["analysis_run", "origin_run"] {
        if input
            .extra
            .get(field)
            .is_some_and(|value| !value.is_string())
        {
            inspection.issue(
                InspectionCode::DependencyUnavailable,
                &run.join("input.json"),
                format!("{field} location is invalid"),
            );
        }
    }
    if input
        .extra
        .get("upstream")
        .is_some_and(|value| !value.is_array())
    {
        inspection.issue(
            InspectionCode::DependencyUnavailable,
            &run.join("input.json"),
            "upstream inventory is invalid",
        );
    }
    if input
        .extra
        .get(PROPOSAL_TARGET)
        .is_some_and(|value| !value.is_boolean())
    {
        inspection.issue(
            InspectionCode::LedgerInvalid,
            &run.join("input.json"),
            "proposal target marker is invalid",
        );
    }
    if let Some(parents) = input
        .extra
        .get("upstream")
        .and_then(serde_json::Value::as_array)
    {
        for parent in parents {
            if parent["bundle"].as_str().is_none() {
                inspection.issue(
                    InspectionCode::DependencyUnavailable,
                    &run.join("input.json"),
                    "ancestor location is missing",
                );
            }
        }
    }
    let mut analysis = None;
    let proposal_target = input.extra.get(PROPOSAL_TARGET) == Some(&serde_json::json!(true));
    let source_matches = if input.state == "referenced" && !proposal_target {
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
    inspect_in(target, stores, Inspection::default())
}

fn inspect_in(
    target: &Path,
    stores: &[PathBuf],
    mut inspection: Inspection,
) -> Result<EvidenceCheck> {
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
            if let Some(parents) = input
                .extra
                .get("upstream")
                .and_then(serde_json::Value::as_array)
            {
                for parent in parents {
                    let Some(path) = parent["bundle"].as_str() else {
                        inspection.issue(
                            InspectionCode::DependencyUnavailable,
                            &run.join("input.json"),
                            "ancestor location is missing",
                        );
                        continue;
                    };
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
    // Only explicit ownership or the runner's declared upstream ledgers may
    // collapse records. Equal names/hashes alone never merge distinct analyses.
    let mut owners = BTreeMap::new();
    for (run, input) in &known_runs {
        if let Some(owner) = input
            .extra
            .get("analysis_run")
            .and_then(serde_json::Value::as_str)
        {
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
        } else if run.join("runtime.json").is_file()
            && let Some(parents) = input
                .extra
                .get("upstream")
                .and_then(serde_json::Value::as_array)
        {
            // Compatibility with registered runs written before analysis_run.
            // A reference-only ledger is dedicated to the declaring operation.
            for parent in parents {
                if let Some(path) = parent["bundle"].as_str() {
                    let ledger = lexical(&run.join(path));
                    if known_runs.iter().any(|(path, _)| lexical(path) == ledger)
                        && !ledger.join("runtime.json").exists()
                    {
                        owners.entry(ledger).or_insert_with(|| run.clone());
                    }
                }
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
        let Some(value) = input.extra.get("origin_run") else {
            return Ok(false);
        };
        let origin = value
            .as_str()
            .ok_or_else(|| invalid("origin run location is invalid"))?;
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
                    if input.state == "preserved" {
                        let (owner, retained) = owning_ledger(&affected.run)?;
                        let effective = retained
                            .source
                            .as_ref()
                            .ok_or_else(|| invalid("reference is missing"))?;
                        verify_preserved(&owner.join("preserved-input"), effective)?;
                        preserved.insert(reference.hash.clone(), owner);
                        continue;
                    }
                    if input.state != "referenced" {
                        return Err(invalid("input evidence changed; refresh the save decision"));
                    }
                    let source = affected.run.join(&reference.path);
                    if !same_source(&affected.run, reference, &conflict.target) {
                        return Err(invalid("input reference moved; refresh the save decision"));
                    }
                    if let Some(owner) = preserved.get(&reference.hash) {
                        verify_preserved(&owner.join("preserved-input"), reference)?;
                        input.extra.insert(
                            "origin_run".into(),
                            serde_json::json!(relative(&affected.run, owner)?),
                        );
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
                    std::fs::rename(&affected.run, destination)?;
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
        let Some(origin) = input
            .extra
            .get("origin_run")
            .and_then(serde_json::Value::as_str)
        else {
            break;
        };
        location = location.join(origin).canonicalize()?;
        if !visited.insert(location.clone()) {
            return Err(invalid("input evidence contains a cycle"));
        }
        input = read_input(&location)?;
    }
    if input.extra.contains_key("origin_run") {
        return Err(invalid("input evidence ancestry exceeds 32 references"));
    }
    Ok((location, input))
}

/// Resolve evidence only on explicit verification/export/relink. No UI-event hashing.
pub fn verified_source(bundle: &Path, reference: &InputReference) -> Result<PathBuf> {
    let (mut location, mut input) = owning_ledger(bundle)?;
    if input.state == "broken" {
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
            if candidate.state == "broken" {
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
    let path = if input.state == "preserved" {
        location.join("preserved-input")
    } else {
        location.join(&effective.path)
    };
    if basecraft_io::hash_file(&path)? != reference.hash
        || std::fs::metadata(&path)?.len() != reference.stamp.size
    {
        return Err(invalid(
            "referenced input changed; original evidence unavailable (timestamp alone is not identity)",
        ));
    }
    Ok(path.canonicalize()?)
}

/// Honest repair: only a byte-verified original may replace a missing/changed reference.
pub fn relink(bundle: &Path, original: &Path) -> Result<()> {
    let _lock = lock()?;
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
        if ledger.state == "broken" || ledger.extra.contains_key("origin_run") {
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
pub fn package(result: &Path, destination: &Path) -> Result<()> {
    let issues = crate::provenance::verify(result)?;
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
    let bundle = crate::provenance::bundle_for_result(result);
    let mut publication = crate::provenance::inspect(result)?;
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
    let target_bundle = crate::provenance::companion(&copied);
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
        let final_bundle = crate::provenance::companion(
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
    // Complete tree exports deliberately materialize the legacy exchange
    // association, so extraction still works with the original store absent.
    if let Ok(tree) = crate::files::load_tree(result)
        && crate::links::metadata_present(&crate::links::sidecar(result))?
    {
        let link = crate::links::AlignmentLink::read(result)?;
        let input_path = link.checked_snapshot(result, &tree.doc)?;
        let indexed = std::sync::Arc::new(basecraft_io::cache::open_fasta(&input_path)?);
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
        let paths = crate::links::AlignmentLink::create(
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
    crate::provenance::publish(&target_bundle, &publication)?;
    let authored = bundle.join("reports/methods.md");
    if authored.is_file() {
        copy(&authored, &target_bundle.join("reports/methods.md"))?;
    }
    let issues = crate::provenance::verify(&copied)?;
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
        let bytes =
            crate::provenance::read_record(&key.0.join("records").join(format!("{}.json", key.1)))?;
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
        mapping.push(
            basecraft_protocol::export_records::ProvenanceExportMapping {
                original_id: old_id.clone(),
                original_hash: hash.clone(),
                export_id: publication.id.clone(),
                extra: BTreeMap::new(),
            },
        );
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
    let bytes = serde_json::to_vec_pretty(&serde_json::to_value(
        basecraft_protocol::export_records::ProvenanceExportMap {
            schema_version: 1,
            records: mapping,
            extra: BTreeMap::new(),
        },
    )?)?;
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
        if let Some(origin) = input
            .extra
            .get("origin_run")
            .and_then(serde_json::Value::as_str)
        {
            stack.push((location.join(origin), false));
        }
        if let Some(upstream) = input
            .extra
            .get("upstream")
            .and_then(serde_json::Value::as_array)
        {
            for reference in upstream {
                if records.len() + stack.len() >= 10_000 {
                    return Err(invalid("evidence ancestry exceeds 10,000 references"));
                }
                let directory = reference["bundle"]
                    .as_str()
                    .ok_or_else(|| invalid("ancestor location is missing"))?;
                let directory = location.join(directory).canonicalize()?;
                let id = reference["id"]
                    .as_str()
                    .ok_or_else(|| invalid("ancestor identity is missing"))?;
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    return Err(invalid("unsafe ancestor identity"));
                }
                stack.push((directory.clone(), false));
                if !seen_records.insert((directory.clone(), id.to_owned())) {
                    continue;
                }
                let bytes = crate::provenance::read_record(
                    &directory.join("records").join(format!("{id}.json")),
                )?;
                bytes_read = bytes_read.saturating_add(bytes.len());
                if bytes_read > 128 << 20 {
                    return Err(invalid("evidence ancestry exceeds 128 MiB"));
                }
                if reference["hash"].as_str() != Some(basecraft_io::hash_bytes(&bytes).as_str()) {
                    return Err(invalid("ancestor record hash differs"));
                }
                let publication: basecraft_protocol::provenance::Publication =
                    serde_json::from_slice(&bytes)?;
                if publication.id != id || publication.schema_version != 1 {
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
        issues.extend(crate::provenance::verify_records(&directory, &publication)?);
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
        let bytes = crate::provenance::read_record(&dependency.join("manifest.json"))?;
        let publication: basecraft_protocol::provenance::Publication =
            serde_json::from_slice(&bytes)?;
        upstream.push(serde_json::json!({"bundle":crate::input_evidence::relative(run.directory(), dependency)?, "id":publication.id,"hash":basecraft_io::hash_bytes(&bytes)}));
    }
    crate::input_evidence::write_json(
        &run.artifact("input.json")?,
        &crate::input_evidence::SubmittedInput {
            schema_version: 1,
            source: None,
            submitted_hash: basecraft_io::hash_bytes(payload),
            submitted_size: payload.len() as u64,
            transformation: transformation.into(),
            state: "generated".into(),
            extra: BTreeMap::from([("upstream".into(), serde_json::json!(upstream))]),
        },
    )?;
    crate::input_evidence::write_json(
        &run.artifact("run.json")?,
        &serde_json::to_value(basecraft_protocol::records::AnalysisRun {
            schema_version: 1,
            id: run.id().into(),
            result: Some(result_label.into()),
            status: basecraft_protocol::records::AnalysisRunStatus::Prepared,
            outputs: None,
            extra: BTreeMap::new(),
        })?,
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
