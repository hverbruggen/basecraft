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
