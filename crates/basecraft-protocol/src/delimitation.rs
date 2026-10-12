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
    Add {
        hypothesis: Hypothesis,
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
