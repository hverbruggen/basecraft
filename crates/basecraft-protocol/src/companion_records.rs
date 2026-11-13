//! A document's companion: what the document's own format cannot hold. It
//! lives in the project's `.basecraft` (or the `.basecraft` beside a loose
//! file) at `documents/<path>.json`, never beside the document (D23). One
//! envelope for every kind, with one section per kind.
use crate::facts::{Alphabet, BranchLengths, Fact};
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
    /// The letters an alignment or sequence is written in, when the user said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alphabet: Option<Fact<Alphabet>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<TreeCompanion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimitation: Option<DelimitationCompanion>,
}

impl Companion {
    /// Write the companion as basecraft keeps it: indented JSON and a final
    /// newline. Every companion is written through here.
    pub fn write(&self, w: &mut dyn std::io::Write) -> std::io::Result<()> {
        w.write_all(&crate::records::encode(self)?)
    }

    pub fn new(exchange_hash: impl Into<String>) -> Self {
        Self {
            version: COMPANION_VERSION,
            exchange_hash: exchange_hash.into(),
            alphabet: None,
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
    /// What the branch lengths measure. Absent means unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_lengths: Option<Fact<BranchLengths>>,
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

impl crate::records::RecordFormat for Companion {
    fn is_current(&self) -> bool {
        self.version == COMPANION_VERSION
    }
}
