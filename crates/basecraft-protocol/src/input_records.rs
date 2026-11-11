//! Submitted-input evidence data. Validation and filesystem effects belong to the host.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provenance::InputReference;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SubmittedInput {
    pub schema_version: u32,
    pub source: Option<InputReference>,
    pub submitted_hash: String,
    pub submitted_size: u64,
    /// canonical-fasta-v1 or ungapped-fasta-v1; generated inputs retain their bytes.
    pub transformation: String,
    pub state: EvidenceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_target: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstream: Vec<UpstreamLink>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Unknown states remain readable and serialize without losing their spelling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Referenced,
    Generated,
    Preserved,
    Broken,
    #[serde(untagged)]
    Unknown(String),
}

impl EvidenceState {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Referenced => "referenced",
            Self::Generated => "generated",
            Self::Preserved => "preserved",
            Self::Broken => "broken",
            Self::Unknown(value) => value,
        }
    }
}
impl From<&str> for EvidenceState {
    fn from(value: &str) -> Self {
        match value {
            "referenced" => Self::Referenced,
            "generated" => Self::Generated,
            "preserved" => Self::Preserved,
            "broken" => Self::Broken,
            _ => Self::Unknown(value.into()),
        }
    }
}
impl From<String> for EvidenceState {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UpstreamLink {
    pub bundle: std::path::PathBuf,
    pub hash: String,
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evidence_state_keeps_unknown_values() {
        for value in [
            "referenced",
            "generated",
            "preserved",
            "broken",
            "future",
            "",
        ] {
            let encoded = serde_json::to_string(value).unwrap();
            let state: EvidenceState = serde_json::from_str(&encoded).unwrap();
            assert_eq!(state.as_str(), value);
            assert_eq!(serde_json::to_string(&state).unwrap(), encoded);
        }
        assert!(matches!(
            EvidenceState::from("future"),
            EvidenceState::Unknown(_)
        ));
    }
}
