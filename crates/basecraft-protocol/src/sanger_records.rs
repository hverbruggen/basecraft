//! Existing saved Sanger review envelopes and embedded evidence.
//! Data only: legacy versions, field order and unknown-field handling are unchanged.
use crate::{provenance::InputReference, sanger::ReviewOp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SavedReview {
    pub version: u32,
    pub draft_path: PathBuf,
    pub draft_sha256: String,
    pub draft_json: String,
    pub events: Vec<ReviewOp>,
    #[serde(default)]
    pub evidence: ReviewEvidence,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedCallSets {
    pub version: u32,
    pub selected: String,
    pub sets: Vec<SavedReview>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReviewEvidence {
    pub schema_version: u32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub runs: Vec<PathBuf>,
    #[serde(default)]
    pub sources: Vec<ReviewSource>,
    pub placement: Option<ReviewSupport>,
}

impl Default for ReviewEvidence {
    fn default() -> Self {
        Self {
            schema_version: 1,
            extra: BTreeMap::new(),
            runs: vec![],
            sources: vec![],
            placement: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReviewSource {
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
    // Identity in the immutable draft/placement, never rebased.
    pub original: PathBuf,
    pub sha256: String,
    // Location relative to the saved review; ledger controls preserve/break decisions.
    pub path: PathBuf,
    pub ledger: Option<PathBuf>,
    pub reference: Option<InputReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReviewSupport {
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
    pub path: PathBuf,
    pub hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer_bytes<T: Serialize>(record: &T) -> Vec<u8> {
        // The existing storage writer serializes through Value, whose map order
        // is part of the captured pretty-JSON byte contract.
        serde_json::to_vec_pretty(&serde_json::to_value(record).unwrap()).unwrap()
    }

    #[test]
    fn captured_saved_reviews_keep_the_exact_writer_bytes() {
        let single = include_bytes!("../../../testdata/fixtures/sanger-legacy/single.json");
        let multi = include_bytes!("../../../testdata/fixtures/sanger-legacy/multi.json");
        let one: SavedReview = serde_json::from_slice(single).unwrap();
        let many: SavedCallSets = serde_json::from_slice(multi).unwrap();
        assert_eq!(writer_bytes(&one), single);
        assert_eq!(writer_bytes(&many), multi);
        assert_eq!(one.version, 1);
        assert_eq!(many.version, 2);
        assert_eq!(many.sets.len(), 2);
        assert!(
            many.sets
                .iter()
                .any(|set| set.draft_sha256 == many.selected)
        );
    }

    #[test]
    fn extension_fields_and_call_set_rejection_keep_legacy_contracts() {
        let mut one: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../testdata/fixtures/sanger-legacy/single.json"
        ))
        .unwrap();
        one["future-review"] = serde_json::json!({"retain": [1, 2]});
        one["evidence"]["future-evidence"] = serde_json::json!({"retain": true});
        let typed: SavedReview = serde_json::from_value(one.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), one);

        let mut many: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../testdata/fixtures/sanger-legacy/multi.json"
        ))
        .unwrap();
        many["future-envelope"] = serde_json::json!(true);
        assert!(serde_json::from_value::<SavedCallSets>(many).is_err());
    }
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
    /// Missing legacy or instrument evidence is unknown, never primary agreement.
    /// Draft orientation and clipping do not transform these native arrays.
    #[serde(default)]
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
