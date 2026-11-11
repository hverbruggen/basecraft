//! Existing Sanger review wire payloads. These do not define the saved-review envelope.
//! Extracted from the Basecraft docs/ops review contracts; serialization is unchanged.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Versioned, explicit rules for automatic review triage, not error probabilities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ReviewSettings {
    pub strong_score: i32,
    pub paired_score: i32,
    /// A secondary peak at or above this percentage is ambiguous.
    pub ambiguity_percent: u32,
    pub allow_weak_disagreement: bool,
    /// Minimum candidate / strongest peak ratio in a non-confident opposing read.
    pub weak_support_percent: u32,
    pub check_reference: bool,
    /// Zero means any observed canonical occurrence; denominator is A/C/G/T rows.
    pub reference_percent: u32,
    pub reference_min_rows: u32,
}
impl Default for ReviewSettings {
    fn default() -> Self {
        Self {
            strong_score: 30,
            paired_score: 20,
            ambiguity_percent: 35,
            allow_weak_disagreement: true,
            weak_support_percent: 80,
            check_reference: true,
            reference_percent: 0,
            reference_min_rows: 1,
        }
    }
}
impl ReviewSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(0..=100).contains(&self.strong_score)
            || !(0..=self.strong_score).contains(&self.paired_score)
            || !(1..=100).contains(&self.ambiguity_percent)
            || !(1..=100).contains(&self.weak_support_percent)
            || self.reference_percent > 100
            || self.reference_min_rows == 0
            || self.reference_min_rows > 1_000_000
        {
            return Err(
                "Invalid review thresholds (paired score must not exceed strong score)".into(),
            );
        }
        Ok(())
    }
}

/// Frozen whole-alignment counts used by a recorded triage operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewReference {
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub counts: Vec<Option<[u32; 4]>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MaskSettings {
    /// Zero-based inclusive start, exclusive end in consensus columns.
    pub start: usize,
    pub end: usize,
    pub forward: String,
    pub reverse: String,
    pub minimum_overlap: usize,
    pub error_percent: usize,
    pub evidence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "choice", content = "base", rename_all = "snake_case")]
pub enum ReviewChoice {
    Pending,
    Accept,
    Auto(char),
    Reject,
    Edit(char),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ReviewOp {
    /// Conservative initial triage; retained in the replay log, outside undo history.
    AutoAccept {
        columns: Vec<usize>,
    },
    /// Re-evaluate automatic calls while preserving all explicit manual decisions.
    /// The rule version and frozen reference counts make this portable and auditable.
    Evaluate {
        version: u32,
        settings: ReviewSettings,
        calls: Vec<Option<char>>,
        reference: Option<ReviewReference>,
    },
    Set {
        columns: Vec<usize>,
        choice: ReviewChoice,
    },
    Mask {
        settings: MaskSettings,
    },
    Export {
        path: PathBuf,
        sha256: String,
    },
}
