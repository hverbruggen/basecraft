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

/// Recovery lifecycle labels. Retain an unfamiliar label without reinterpreting it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStatus {
    Unsaved,
    Pending,
    #[serde(untagged)]
    Unknown(String),
}

/// `recovery.json` in the session's recovery directory.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RecoveryManifest {
    pub schema_version: u32,
    pub session: String,
    /// Original source root, retained verbatim; this is local recovery state.
    pub source_root: PathBuf,
    pub status: RecoveryStatus,
    /// An existing `LogEntry` snapshot. Keep its JSON intact, including unknown
    /// nested fields: deserializing into `LogEntry` would currently discard them.
    /// The schema reuses the shared current log type rather than duplicating it.
    #[schemars(with = "crate::LogEntry")]
    pub baseline: Value,
    pub object_policy_limit: u64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// `pending-save.json`; data and provenance remain separate writes.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PendingSave {
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
    /// The public invocation context rejects unknown fields, including in input
    /// evidence. Retain its complete snapshot for lossless nested extensions;
    /// production still constructs this from the shared `InvocationContext`.
    #[schemars(with = "crate::operation::InvocationContext")]
    pub context: Value,
    pub requested_parameters: Value,
    pub effective_parameters: Value,
    pub stdout: String,
    pub stderr: String,
    /// `ToolInfo` currently discards extras. Keep the original snapshot; null is
    /// always emitted, including on process failure. No production parse changes.
    #[schemars(with = "Option<crate::plugin::ToolInfo>")]
    pub tool: Option<Value>,
    /// `ExecutionInfo` and `MethodsInfo` currently discard extras. Preserve nested
    /// process evidence rather than introducing a lossy parsing boundary.
    #[schemars(with = "Option<crate::plugin::ExecutionInfo>")]
    pub execution: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The existing two-field plugin identity in the local index. Neither `Actor`'s
/// tagged representation nor `OperationPins` has this shape.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobIndexPlugin {
    pub id: String,
    pub version: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Existing display path wrapper; not a hashed `InputReference` or a new path rule.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobIndexSource {
    pub path: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
    #[serde(untagged)]
    Unknown(String),
}

/// The existing version-one analysis-store `run.json` family.
/// This describes fresh writer shapes; production readers retain their selective
/// JSON checks. Serialize through Value for recursive sorting before write_json.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisRun {
    pub schema_version: u32,
    pub id: String,
    /// Always emitted, including null for a non-UTF-8 reference anchor filename.
    pub result: Option<String>,
    pub status: AnalysisRunStatus,
    /// Absent until referenced publication; a completed run can have an empty
    /// array. Reuse ArtifactRevision's schema, retaining its JSON snapshot because
    /// the public type rejects unknown extensions on deserialization. Fresh
    /// writers still construct these snapshots from the public ArtifactRevision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<crate::provenance::ArtifactRevision>>")]
    pub outputs: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
