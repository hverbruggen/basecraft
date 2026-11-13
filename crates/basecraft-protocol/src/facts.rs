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
    /// No letters to judge by: gaps, N, X and ? only.
    #[default]
    Unknown,
    /// A guess only: a few protein-only letters, 1% or fewer. The user has
    /// to say what the sequence is before anything that needs it runs.
    Undetermined,
    /// A document only: its sequences are not all of one kind.
    Mixed,
}

/// Letter counts for basecraft's one guess (D12), so a long sequence can be
/// read in pieces. Gaps, N, X and ? don't count.
#[derive(Clone, Copy, Debug, Default)]
pub struct AlphabetCounts {
    letters: u64,
    protein_only: u64,
    t: bool,
    u: bool,
}

impl AlphabetCounts {
    pub fn add(&mut self, residues: &[u8]) {
        for &b in residues {
            let b = b.to_ascii_uppercase();
            if matches!(b, b'N' | b'X') || !(b.is_ascii_alphabetic() || b == b'*') {
                continue;
            }
            self.letters += 1;
            match b {
                b'E' | b'F' | b'I' | b'L' | b'P' | b'Q' | b'J' | b'O' | b'Z' | b'*' => {
                    self.protein_only += 1;
                }
                b'T' => self.t = true,
                b'U' => self.u = true,
                _ => {}
            }
        }
    }

    /// No protein-only letters: DNA, or RNA when U appears without T (IUPAC
    /// ambiguity codes count as nucleotides). Protein-only letters above 1%
    /// of the counted letters: protein. Fewer: undetermined.
    pub fn alphabet(&self) -> Alphabet {
        if self.letters == 0 {
            Alphabet::Unknown
        } else if self.protein_only == 0 {
            if self.u && !self.t {
                Alphabet::Rna
            } else {
                Alphabet::Dna
            }
        } else if self.protein_only * 100 > self.letters {
            Alphabet::Protein
        } else {
            Alphabet::Undetermined
        }
    }
}

impl Alphabet {
    /// Basecraft's one guess for one sequence (D12): see [`AlphabetCounts`].
    pub fn guess(residues: &[u8]) -> Self {
        let mut counts = AlphabetCounts::default();
        counts.add(residues);
        counts.alphabet()
    }

    /// A document's alphabet from its sequences': their common value, or
    /// mixed when they differ. Sequences with no letters don't count.
    pub fn common(values: impl IntoIterator<Item = Alphabet>) -> Self {
        let mut found = Alphabet::Unknown;
        for value in values.into_iter().filter(|v| *v != Alphabet::Unknown) {
            if found == Alphabet::Unknown {
                found = value;
            } else if found != value {
                return Alphabet::Mixed;
            }
        }
        found
    }

    /// A guess as a fact, for a document whose side file states nothing.
    pub fn guessed(value: Alphabet) -> Fact<Self> {
        Fact {
            value,
            source: FactSource::Guessed,
        }
    }
}

/// One sequence's guessed alphabet, as reported for a document whose
/// sequences are not all of one kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SequenceAlphabet {
    pub name: String,
    pub alphabet: Alphabet,
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

    fn with(base: &[u8], extra: &[u8], count: usize) -> Vec<u8> {
        base.iter()
            .copied()
            .cycle()
            .take(count)
            .chain(extra.iter().copied())
            .collect()
    }

    #[test]
    fn the_guess_follows_d12() {
        assert_eq!(Alphabet::guess(b"ACGTACGTX--N?"), Alphabet::Dna);
        assert_eq!(Alphabet::guess(b"ACGRYKMSWBDHVN"), Alphabet::Dna);
        assert_eq!(Alphabet::guess(b"acguacgu"), Alphabet::Rna);
        assert_eq!(Alphabet::guess(b"ACGUT"), Alphabet::Dna);
        assert_eq!(
            Alphabet::guess(&with(b"ACGT", b"E", 999)),
            Alphabet::Undetermined
        );
        assert_eq!(
            Alphabet::guess(&with(b"ACGT", b"EE", 98)),
            Alphabet::Protein
        );
        assert_eq!(Alphabet::guess(b"MKLVEFIQ*"), Alphabet::Protein);
        assert_eq!(Alphabet::guess(b"--??NNXX"), Alphabet::Unknown);
    }

    #[test]
    fn a_document_has_an_alphabet_only_when_its_sequences_agree() {
        use Alphabet::{Dna, Mixed, Protein, Unknown};
        assert_eq!(Alphabet::common([Dna, Unknown, Dna]), Dna);
        assert_eq!(Alphabet::common([Dna, Protein]), Mixed);
        assert_eq!(Alphabet::common([]), Unknown);
    }
}
