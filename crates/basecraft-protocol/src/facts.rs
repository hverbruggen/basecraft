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

impl Alphabet {
    /// Basecraft's one guess from residues: protein when any letter falls
    /// outside the IUPAC nucleotide codes, DNA when every letter is one of
    /// them, unknown with no letters. RNA is never guessed.
    pub fn guess(residues: &[u8]) -> Self {
        let mut letters = residues.iter().filter(|b| b.is_ascii_alphabetic()).peekable();
        if letters.peek().is_none() {
            return Self::Unknown;
        }
        if letters.any(|b| !b"ACGTUNRYKMSWBDHV".contains(&b.to_ascii_uppercase())) {
            Self::Protein
        } else {
            Self::Dna
        }
    }

    /// The guess as a fact, for a document whose side file states nothing.
    pub fn guessed(residues: &[u8]) -> Fact<Self> {
        Fact {
            value: Self::guess(residues),
            source: FactSource::Guessed,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::Alphabet;

    #[test]
    fn guess_reads_letters_only() {
        assert_eq!(Alphabet::guess(b"acgt-NRYKMSW"), Alphabet::Dna);
        assert_eq!(Alphabet::guess(b"ACGU"), Alphabet::Dna);
        assert_eq!(Alphabet::guess(b"MKLV*"), Alphabet::Protein);
        assert_eq!(Alphabet::guess(b"ACGTX"), Alphabet::Protein);
        assert_eq!(Alphabet::guess(b"--??"), Alphabet::Unknown);
    }
}
