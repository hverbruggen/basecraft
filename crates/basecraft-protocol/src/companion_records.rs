//! The companion file `<file>.basecraft.json` beside a document: what the
//! document's own format cannot hold. One envelope for every kind, with one
//! section per kind.
use crate::{CladeAnnotation, TreeStyle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// The companion layout this build reads and writes. No other is read.
pub const COMPANION_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Companion {
    pub version: u32,
    /// Hash of the described file's bytes. A different hash means the file
    /// changed outside basecraft, and the companion no longer applies.
    pub exchange_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<TreeCompanion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimitation: Option<DelimitationCompanion>,
}

impl Companion {
    pub fn new(exchange_hash: impl Into<String>) -> Self {
        Self {
            version: COMPANION_VERSION,
            exchange_hash: exchange_hash.into(),
            tree: None,
            delimitation: None,
        }
    }
}

/// What a tree's Newick can't hold.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TreeCompanion {
    /// Canonical topology, raw support and node ordering of the first tree.
    pub tree_hash: String,
    /// Support set names and sources, in the order their values are joined
    /// in the Newick labels.
    pub supports: Vec<SupportName>,
    pub annotations: BTreeMap<u32, CladeAnnotation>,
    pub style: TreeStyle,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SupportName {
    pub name: String,
    pub source: String,
    pub kind: crate::SupportKind,
}

/// A species workspace in full; the SPART file holds only part of it.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DelimitationCompanion {
    /// The folder the workspace's paths were written under. When the SPART
    /// file has moved, paths under it move with it. None leaves them as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_base: Option<PathBuf>,
    pub workspace: crate::delimitation::DelimitationWorkspace,
}
