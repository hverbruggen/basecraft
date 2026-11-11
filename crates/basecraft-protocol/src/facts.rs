//! Facts about a document that its standard file format cannot hold: what
//! its letters are, what a tree's branch lengths measure, whether a tree is
//! rooted. Basecraft keeps them in the document's companion file.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The letters a sequence or alignment is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Alphabet {
    Dna,
    Rna,
    Protein,
    #[default]
    Unknown,
}

/// What a tree's branch lengths measure.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BranchLengths {
    #[default]
    Unknown,
    /// Expected substitutions per site.
    Substitutions,
    /// Time with no unit: the root is 1, or depths are only comparable.
    RelativeTime,
    /// Time in the stated unit, such as "Ma".
    AbsoluteTime { unit: String },
}

/// Whether a tree's root is a real root or only where the file put it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Rooting {
    #[default]
    Unknown,
    Rooted,
    Unrooted,
}

/// Who said so.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FactSource {
    /// The file or the program that made it.
    Stated,
    /// Basecraft, from the content. Can be wrong.
    Guessed,
    /// The user.
    User,
}

/// A fact and who said so, such as `Fact<Alphabet>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Fact<T> {
    pub value: T,
    pub source: FactSource,
}
