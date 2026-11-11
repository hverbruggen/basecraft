//! Portable provenance version 1. Paths inside bundles are relative and never executable.
use crate::{LogEntry, Op};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRevision {
    pub id: String,
    pub logical_name: String,
    pub hash: String,
    pub size: u64,
    /// Relative path inside the bundle; None means bytes are unavailable.
    pub retained: Option<String>,
    /// Original disk evidence. Relative to the publication bundle; never a write destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<InputReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditActivity {
    pub id: String,
    pub operation: Op,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // Independent completeness dimensions, not lifecycle states.
pub struct Completeness {
    pub ancestry_known: bool,
    pub input_bytes_available: bool,
    pub execution_specification_available: bool,
    /// Saving is not a replay verification.
    pub replay_verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub writer: std::collections::BTreeMap<String, u32>,
    pub id: String,
    pub session: String,
    pub parents: Vec<String>,
    #[serde(default)]
    pub parent_hashes: std::collections::BTreeMap<String, String>,
    pub created: String,
    pub origin: String,
    pub inputs: Vec<ArtifactRevision>,
    pub outputs: Vec<ArtifactRevision>,
    /// Full chronology, including undo/redo and abandoned branches.
    pub events: Vec<LogEntry>,
    /// Operations actually contributing to this revision, in causal order.
    pub effective_edits: Vec<EditActivity>,
    pub completeness: Completeness,
    pub report_template_version: u32,
}

/// Cheap change signal captured during an existing verified read. Matching time
/// and length alone do not prove content identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileStamp {
    pub size: u64,
    pub modified_seconds: u64,
    pub modified_nanos: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InputReference {
    pub path: String,
    pub stamp: FileStamp,
    /// Source bytes hash reused from the verified document read.
    pub hash: String,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}
