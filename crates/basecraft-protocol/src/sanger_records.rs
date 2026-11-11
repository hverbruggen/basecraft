//! Current saved Sanger review envelope and embedded evidence.
use crate::{provenance::InputReference, sanger::ReviewOp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedReview {
    pub draft_path: PathBuf,
    pub draft_sha256: String,
    pub draft_json: String,
    pub events: Vec<ReviewOp>,
    pub evidence: ReviewEvidence,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedCallSets {
    pub version: u32,
    pub selected: String,
    pub sets: Vec<SavedReview>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ReviewSupport {
    pub path: PathBuf,
    pub hash: String,
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
            let record: SavedCallSets = serde_json::from_str(bytes).unwrap();
            assert_eq!(record.version, 3);
            assert_eq!(
                serde_json::to_value(record).unwrap(),
                serde_json::from_str::<serde_json::Value>(bytes).unwrap()
            );
        }
    }
}

/// Immutable analysis data stored separately from edit-sized journal entries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewRecord {
    pub path: PathBuf,
    pub hash: String,
}
