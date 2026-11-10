//! Existing recovery and analysis-store records. These describe the v1 wire shape, not a new format.
//!
//! Writers serialize through `serde_json::Value`, as the original literal
//! builders did, sorting keys recursively. Recovery uses pretty JSON without a
//! newline; the store marker uses compact JSON plus a newline, and the registry
//! uses pretty JSON plus a newline. Direct struct serialization is not that
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
