//! Existing recovery, analysis-store and execution records: v1 wire shapes, not new formats.
//!
//! Writers serialize through `serde_json::Value`, as the original literal
//! builders did, sorting keys recursively. Recovery uses pretty JSON without a
//! newline; the store marker uses compact JSON plus a newline, and the registry
//! and execution/index records use pretty JSON plus a newline. Direct struct serialization is not that
//! byte-level contract.
use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Current pending-save lifecycle label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStatus {
    Pending,
}

/// `pending-save.json`; data and provenance remain separate writes.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PendingSave {
    pub baseline: crate::LogEntry,
    pub schema_version: u32,
    pub session: String,
    pub target: PathBuf,
    /// Always emitted, including JSON null when the target did not exist.
    pub before_hash: Option<String>,
    pub expected_hashes: Vec<String>,
    pub status: RecoveryStatus,
    pub note: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Namespace identity; foreign markers must never be claimed or migrated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum StoreFormat {
    #[serde(rename = "basecraft.analysis-store")]
    AnalysisStore,
}

/// `store.json` in an existing `analyses-v1` namespace. Runtime readers also
/// enforce version 1, ordinary-file safety and the existing 4096-byte limit.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct StoreMarker {
    pub format: StoreFormat,
    pub schema_version: u32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Version-one `analysis-stores.json` in application state. These are LOCAL
/// absolute machine paths, not portable project references. Runtime readers
/// enforce version 1, the 1 MiB file bound and at most 4096 absolute paths.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct StoreRegistry {
    pub schema_version: u32,
    pub stores: Vec<PathBuf>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// External-process `execution.json`, distinct from the runtime outcome and the
/// host-workflow envelope. No new reader/version gate is implied by this type.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExternalExecution {
    pub schema: u32,
    pub operation: String,
    pub package_version: String,
    pub package_digest: String,
    pub descriptor_digest: String,
    /// Snapshot of the host's managed-runtime receipt, or explicit null. The
    /// implementation type is GPL host-owned; protocol must not depend on it.
    pub runtime: Option<Value>,
    pub interpreter: Option<PathBuf>,
    /// Existing caller label, not an `Actor` serialization or a new identity.
    pub initiator: String,
    pub executor: String,
    pub process_instance: String,
    pub context: crate::operation::InvocationContext,
    pub requested_parameters: Value,
    pub effective_parameters: Value,
    pub stdout: String,
    pub stderr: String,
    pub tool: Option<crate::plugin::ToolInfo>,
    pub execution: Option<crate::plugin::ExecutionInfo>,
}

/// Host-workflow `execution.json`. This explicit shape intentionally has no
/// external operation, context, stdout/stderr, runtime, interpreter or tool keys.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct HostWorkflowExecution {
    pub schema: u32,
    pub workflow: String,
    pub initiator: String,
    pub executor: String,
    pub requested_parameters: Value,
    pub effective_parameters: Value,
    pub package_digest: String,
    pub descriptor_digest: String,
    pub process_instance: String,
}

/// The existing two-field plugin identity in the local index. Neither `Actor`'s
/// tagged representation nor `OperationPins` has this shape.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobIndexPlugin {
    pub id: String,
    pub version: String,
}

/// Existing display path wrapper; not a hashed `InputReference` or a new path rule.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobIndexSource {
    pub path: String,
}

/// Lightweight application-state `jobs/<id>/manifest.json`. The pointed-to
/// `OperationOutcome` in runtime.json stays authoritative; this is not `JobRecord`.
/// Existing selected-field readers and owner/receipt checks remain unchanged.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LocalJobIndex {
    pub schema_version: u32,
    pub id: String,
    pub status: crate::operation::RuntimeState,
    pub owner_instance: String,
    pub started: String,
    /// Explicit null for nonterminal states; never omitted by the writer.
    pub ended: Option<String>,
    pub plugin: JobIndexPlugin,
    pub operation: String,
    pub effective_parameters: Value,
    pub initiator: String,
    pub source: JobIndexSource,
    pub outputs: BTreeMap<String, String>,
    /// First runtime error message or explicit null.
    pub diagnostics: Option<String>,
    pub runtime_record: PathBuf,
}

/// Existing rollback-durability poison marker. Recovery refuses its presence,
/// regardless of its contents; this model does not replace that safety check.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RecoveryIntegrityError {
    pub schema_version: u32,
    pub verified_length: u64,
    /// Existing human-readable diagnostic, not a new machine error code.
    pub error: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Existing hidden association written by `ArtifactRun::bind`. Runtime readers
/// retain their selective version, namespace, path and content-hash checks.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResultBinding {
    pub schema_version: u32,
    pub run: String,
    /// UTF-8 relative filename, using the existing writer's path conversion.
    pub result: String,
    pub result_hash: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Labels in `run.json`, distinct from the registered operation runtime lifecycle.
/// Unfamiliar labels survive a typed round-trip without being reinterpreted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisRunStatus {
    Referenced,
    Prepared,
    Saved,
    Completed,
}

/// The existing version-one analysis-store `run.json` family.
/// This describes fresh writer shapes; production readers retain their selective
/// JSON checks. Serialize through `Value` for recursive sorting before `write_json`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisRun {
    pub schema_version: u32,
    pub id: String,
    /// Always emitted, including null for a non-UTF-8 reference anchor filename.
    pub result: Option<String>,
    pub status: AnalysisRunStatus,
    /// Absent until referenced publication; a completed run can have an empty
    /// array. Reuse `ArtifactRevision`'s schema, retaining its JSON snapshot because
    /// the public type rejects unknown extensions on deserialization. Fresh
    /// writers still construct these snapshots from the public `ArtifactRevision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<crate::provenance::ArtifactRevision>>")]
    pub outputs: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Existing tree–alignment association, schema 1 (snapshot) or 2 (reference).
/// Field order and direct pretty serialization are part of the existing wire shape.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AlignmentLink {
    pub schema: u32,
    pub source: PathBuf,
    pub source_hash: String,
    pub snapshot: PathBuf,
    pub snapshot_hash: String,
    pub alignment_version: u64,
    /// False for an explicitly associated imported tree: matching names is not inference provenance.
    pub inferred: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_run: Option<PathBuf>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Current source catalogue. Per-source shards hold the affected store closure.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IndexFile<T> {
    pub checksum: String,
    pub value: T,
}

/// Local source and store locations, without unrelated run records.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisInputIndex {
    pub schema_version: u32,
    pub stores: BTreeMap<PathBuf, String>,
    pub sources: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IndexedAnalysisStore {
    pub identity: String,
    pub sources: Vec<PathBuf>,
    pub dependencies: Vec<PathBuf>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Checksummed JSONL envelope. Its flattened entry already carries log version `v`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct JournalEntry<T = crate::LogEntry> {
    #[serde(rename = "_checksum")]
    pub checksum: String,
    #[serde(rename = "_sequence")]
    pub sequence: u64,
    #[serde(flatten)]
    pub entry: T,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowFailure {
    pub version: u32,
    pub job: WorkflowFailureJob,
    pub workspace: Option<crate::delimitation::DelimitationWorkspace>,
    pub metrics: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowFailureJob {
    pub status: crate::delimitation::JobStatus,
    pub diagnostics: Vec<String>,
}

/// Embedded provenance for an activity's exact submitted input.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ActivityInputRun {
    pub schema_version: u32,
    pub id: String,
    pub hash: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Sources a pending ledger change can introduce. None means coverage is unknown.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IndexChange {
    pub store: PathBuf,
    pub sources: Option<std::collections::BTreeSet<PathBuf>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ActivityContributions {
    pub schema_version: u32,
    pub edits: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExtractionRecord {
    pub schema_version: u32,
    pub source_revision: String,
    #[schemars(with = "serde_json::Value")]
    pub selection: crate::Op,
    pub coordinates: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
