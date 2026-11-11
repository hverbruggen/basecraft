//! Existing tree and species-workspace companion envelopes.
use crate::{CladeAnnotation, TreeStyle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// What a tree's Newick can't hold.
#[derive(Default, Serialize, Deserialize, JsonSchema)]
pub struct TreeSidecar {
    #[serde(default)]
    pub schema_version: u32,
    /// Canonical topology, raw support and node ordering of the first tree.
    #[serde(default)]
    pub tree_hash: String,
    /// Exact exchange bytes, including every forest component, when file-backed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange_hash: Option<String>,
    /// Support set names and sources, in the order their values are joined
    /// in the Newick labels.
    #[serde(default)]
    pub supports: Vec<SupportName>,
    #[serde(default)]
    pub annotations: BTreeMap<u32, CladeAnnotation>,
    #[serde(default)]
    pub style: TreeStyle,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SupportName {
    pub name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub kind: crate::SupportKind,
}

/// A portable SPART file plus its versioned Basecraft workspace sidecar.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DelimitationSidecar {
    pub version: u32,
    pub exchange_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_base: Option<PathBuf>,
    pub workspace: crate::delimitation::DelimitationWorkspace,
}
