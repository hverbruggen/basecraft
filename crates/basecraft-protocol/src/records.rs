//! Existing recovery records. These describe the v1 wire shape, not a new format.
//!
//! The session writer serializes through `serde_json::Value`, as its original
//! literal builders did: keys at every level are sorted and there is no final
//! newline. Serializing a struct directly is not that byte-level contract.
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
