//! Submitted-input evidence data. Validation and filesystem effects belong to the host.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provenance::InputReference;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunInput {
    pub schema_version: u32,
    pub source: Option<InputReference>,
    pub submitted_hash: String,
    pub submitted_size: u64,
    pub transformation: Transformation,
    pub state: InputState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_target: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstream: Vec<UpstreamLink>,
    /// The earlier run, relative to this one, that holds the preserved original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_run: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// What became of the file a run read: still referenced where it was, made
/// by basecraft rather than read from a file, kept as a copy beside the run
/// after the file changed, or marked broken by the user's answer to a save.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputState {
    Referenced,
    Generated,
    Preserved,
    Broken,
}

/// How the bytes a run was given were made from its input: the file's own
/// bytes (`identity-v1`), canonical or ungapped FASTA, canonical Newick, a
/// prepared BLAST query, or a sequence snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Transformation {
    #[serde(rename = "identity-v1")]
    Identity,
    #[serde(rename = "canonical-fasta-v1")]
    CanonicalFasta,
    #[serde(rename = "ungapped-fasta-v1")]
    UngappedFasta,
    #[serde(rename = "canonical-newick-v1")]
    CanonicalNewick,
    #[serde(rename = "blast-query-preparation-v1")]
    BlastQueryPreparation,
    #[serde(rename = "sequence-arrow-buffers-v1")]
    SequenceArrowBuffers,
}

impl Transformation {
    /// The name saved records use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity-v1",
            Self::CanonicalFasta => "canonical-fasta-v1",
            Self::UngappedFasta => "ungapped-fasta-v1",
            Self::CanonicalNewick => "canonical-newick-v1",
            Self::BlastQueryPreparation => "blast-query-preparation-v1",
            Self::SequenceArrowBuffers => "sequence-arrow-buffers-v1",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UpstreamLink {
    pub bundle: std::path::PathBuf,
    pub hash: String,
    pub id: String,
}
