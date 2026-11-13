//! Current saved Sanger review envelope and embedded evidence.
use crate::{provenance::InputReference, sanger::ReviewOp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SavedReview {
    pub draft_path: PathBuf,
    pub draft_sha256: String,
    pub draft_json: String,
    pub events: Vec<ReviewOp>,
    pub evidence: ReviewEvidence,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SavedCallSets {
    pub version: u32,
    pub selected: String,
    pub sets: Vec<SavedReview>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewEvidence {
    pub schema_version: u32,
    pub runs: Vec<PathBuf>,
    pub sources: Vec<ReviewSource>,
    pub placement: Option<ReviewSupport>,
}

impl Default for ReviewEvidence {
    fn default() -> Self {
        Self {
            schema_version: 1,
            runs: vec![],
            sources: vec![],
            placement: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewSource {
    // Identity in the immutable draft/placement, never rebased.
    pub original: PathBuf,
    pub sha256: String,
    // Location relative to the saved review; ledger controls preserve/break decisions.
    pub path: PathBuf,
    pub ledger: Option<PathBuf>,
    pub reference: Option<InputReference>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewSupport {
    pub path: PathBuf,
    pub hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SangerRequest {
    pub operation: String,
    pub sources: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SangerJobStatus {
    Running,
    Failed,
    Cancelled,
    Interrupted,
    SupportRetained,
    ReviewRequiresAttention,
    #[serde(untagged)]
    Unknown(String),
}

/// Execution and job-index share this shape. Recovery omits `artifact_run`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SangerJobRecord {
    pub schema_version: u32,
    pub id: String,
    pub owner_instance: String,
    pub status: SangerJobStatus,
    pub operation: String,
    pub started: String,
    pub source: SangerJobSource,
    pub outputs: BTreeMap<String, String>,
    pub primary_output: Option<PathBuf>,
    pub diagnostics: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_run: Option<PathBuf>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SangerJobSource {
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TraceCalls {
    pub sequence: String,
    /// Native secondary calls, indexed by the same events as sequence/positions/scores.
    /// Absent when the instrument gave none; that is unknown, never primary agreement.
    /// Draft orientation and clipping do not transform these native arrays.
    pub secondary_sequence: Option<String>,
    pub positions: Vec<u32>,
    pub scores: Vec<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_bounds: Option<[usize; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InputRead {
    pub read: usize,
    pub path: PathBuf,
    pub sha256: String,
    pub orientation: Orientation,
    #[serde(default)]
    pub calls: Option<TraceCalls>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Forward,
    Reverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    Call,
    InternalGap,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReadSupport {
    pub read: usize,
    pub base: char,
    pub kind: SupportKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DraftColumn {
    /// One-based column in the Tracy alignment, including gap columns.
    pub column: usize,
    pub consensus: char,
    pub support: Vec<ReadSupport>,
    pub conflict: bool,
    pub review: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Overlap {
    pub reads: [usize; 2],
    pub comparable_bases: usize,
    pub identity: f64,
    pub accepted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SangerDraft {
    pub status: String,
    pub engine: String,
    #[serde(default)]
    pub signal_threshold: Option<u32>,
    pub version_output: String,
    pub engine_sha256: String,
    pub mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    pub inputs: Vec<InputRead>,
    pub consensus: String,
    pub consensus_length: usize,
    pub minimum_overlap: usize,
    pub minimum_identity: f64,
    pub overlaps: Vec<Overlap>,
    pub aligned_columns: usize,
    pub conflict_columns: usize,
    pub columns: Vec<DraftColumn>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SangerPlacement {
    pub status: String,
    pub engine: String,
    pub version: String,
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub source_rows: usize,
    pub source_width: usize,
    pub fragment: String,
    pub map: Vec<Option<usize>>,
    pub aligned_probe: String,
    pub insertions: usize,
    pub parameters: Vec<String>,
}

/// The saved review format this build writes.
pub const SAVED_REVIEW_VERSION: u32 = 3;

impl crate::records::RecordFormat for SavedCallSets {
    fn is_current(&self) -> bool {
        self.version == SAVED_REVIEW_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_envelopes_round_trip() {
        for bytes in [
            include_str!("../../../testdata/fixtures/sanger-review/single.json"),
            include_str!("../../../testdata/fixtures/sanger-review/multi.json"),
        ] {
            let record: SavedCallSets = crate::records::decode(bytes.as_bytes()).unwrap();
            assert_eq!(record.version, 3);
            // The record type does not hold the stamp; everything else survives.
            let mut original: serde_json::Value = serde_json::from_str(bytes).unwrap();
            original
                .as_object_mut()
                .unwrap()
                .remove(crate::records::WRITTEN_BY);
            assert_eq!(serde_json::to_value(record).unwrap(), original);
        }
    }
}

/// Immutable analysis data stored separately from edit-sized journal entries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewRecord {
    pub path: PathBuf,
    pub hash: String,
}
