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
