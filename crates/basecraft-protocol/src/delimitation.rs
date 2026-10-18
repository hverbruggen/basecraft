//! Portable species-hypothesis contracts. No implementation dependencies.
use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// IDs refer to source records, never current tree row positions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Specimen {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Score {
    pub value: f64,
    pub method: String,
    pub scale: String,
    pub interpretation: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub color: String,
    pub members: BTreeSet<String>,
    #[serde(default)]
    pub scores: Vec<Score>,
    #[serde(default)]
    pub individual_scores: BTreeMap<String, Vec<Score>>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Completed,
    Failed,
    Cancelled,
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    pub method: String,
    pub version: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    pub analysis_tree: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    pub seed: Option<u64>,
    pub workers: Option<usize>,
    pub status: Option<JobStatus>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
    /// Representative ID to original specimen IDs; never likelihood weights.
    #[serde(default)]
    pub haplotypes: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Hypothesis {
    pub id: String,
    pub name: String,
    pub origin: HypothesisOrigin,
    pub parent: Option<String>,
    pub groups: Vec<Group>,
    pub unassigned: BTreeSet<String>,
    #[serde(default)]
    pub scores: Vec<Score>,
    pub provenance: Provenance,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisOrigin {
    Manual,
    Imported,
    Inferred,
    Derived,
    ManualCut,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DelimitationView {
    pub display_tree: Option<String>,
    pub visible: Vec<String>,
    pub selected: BTreeSet<String>,
    /// Hypothesis ID to its bar column style.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub columns: BTreeMap<String, ColumnStyle>,
    /// Display-tree clades drawn flipped. Each is named by two tips whose
    /// most recent common ancestor is the clade, whatever the drawing order.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub flipped: BTreeSet<Vec<String>>,
}

/// How one hypothesis column is drawn. Unset fields follow the theme.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnStyle {
    /// Bar width in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Pixels between this column and the next. Unset: 1.5 bar widths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<u32>,
    /// Pixels between one species' bar and the next down the column.
    /// Unset: 0.65 bar widths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<u32>,
    /// Group ID to `#rrggbb`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, String>,
    /// Write the species names to the right of the bars.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub names: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DelimitationWorkspace {
    pub version: u32,
    pub name: String,
    pub specimens: Vec<Specimen>,
    pub hypotheses: Vec<Hypothesis>,
    #[serde(default)]
    pub view: DelimitationView,
    /// Unsupported interchange fields retained verbatim when possible.
    #[serde(default)]
    pub exchange_fields: BTreeMap<String, String>,
}

/// One atomic edit. Inferred and imported results must first be derived.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DelimitationEdit {
    /// Import a validated batch without splitting lineage across separate edits.
    Combine {
        hypotheses: Vec<Hypothesis>,
    },
    Add {
        hypothesis: Hypothesis,
    },
    /// Drop hypotheses. Refused while a kept hypothesis was copied from one.
    Remove {
        hypotheses: BTreeSet<String>,
    },
    Derive {
        source: String,
        id: String,
        name: String,
    },
    Rename {
        hypothesis: String,
        name: String,
    },
    RenameGroup {
        hypothesis: String,
        group: String,
        label: String,
    },
    Split {
        hypothesis: String,
        group: Group,
    },
    Merge {
        hypothesis: String,
        groups: BTreeSet<String>,
        id: String,
        label: String,
    },
    Reassign {
        hypothesis: String,
        specimens: BTreeSet<String>,
        group: Option<String>,
    },
    SaveCut {
        hypothesis: Hypothesis,
        threshold: f64,
        boundary: String,
    },
    SetView {
        view: DelimitationView,
    },
    SetDisplayTree {
        path: String,
        hash: String,
        evidence_run: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobRecord {
    pub id: String,
    pub generation: u64,
    pub status: JobStatus,
    pub method: String,
    pub inputs: BTreeMap<String, String>,
    pub outputs: BTreeMap<String, String>,
    pub diagnostics: Vec<String>,
    pub elapsed_seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DelimitationRequest {
    pub version: u32,
    pub job_id: String,
    pub generation: u64,
    pub method: String,
    pub input: String,
    pub input_hash: String,
    pub output: String,
    pub rooted: bool,
    pub tree_index: usize,
    pub seed: u64,
    pub workers: usize,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    pub progress: Option<String>,
    pub cancel_file: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DelimitationResult {
    pub version: u32,
    pub job: JobRecord,
    pub workspace: Option<DelimitationWorkspace>,
    pub metrics: BTreeMap<String, serde_json::Value>,
}
