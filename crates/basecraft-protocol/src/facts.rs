//! Facts about a document that its standard file format cannot hold: what
//! its letters are, what a tree's branch lengths measure, whether a tree is
//! rooted. Basecraft keeps them in the document's companion file.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The letters a sequence or alignment is written in.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
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

impl std::fmt::Display for Alphabet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Alphabet::Dna => "DNA",
            Alphabet::Rna => "RNA",
            Alphabet::Protein => "protein",
            Alphabet::Unknown => "unknown",
            Alphabet::Undetermined => "undetermined",
            Alphabet::Mixed => "mixed",
        })
    }
}

/// What a user can state: a kind of sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatedAlphabet {
    Dna,
    Rna,
    Protein,
}

impl From<StatedAlphabet> for Alphabet {
    fn from(stated: StatedAlphabet) -> Self {
        match stated {
            StatedAlphabet::Dna => Alphabet::Dna,
            StatedAlphabet::Rna => Alphabet::Rna,
            StatedAlphabet::Protein => Alphabet::Protein,
        }
    }
}

/// Letters counted per sequence when a document opens. `u64::MAX` counts
/// every letter; a cap on the guess (a pending user decision) is this one
/// number. Edits always update the counts exactly.
pub const GUESS_LETTERS: u64 = u64::MAX;

/// Letter counts for basecraft's one guess (D12). Edits add what they insert
/// and remove what they delete, so the guess stays current at the cost of
/// the edit. Gaps, N, X and ? don't count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlphabetCounts {
    letters: u64,
    protein_only: u64,
    t: u64,
    u: u64,
}

impl AlphabetCounts {
    pub fn of(residues: &[u8]) -> Self {
        let mut counts = Self::default();
        counts.add(residues);
        counts
    }

    pub fn letters(&self) -> u64 {
        self.letters
    }

    pub fn add(&mut self, residues: &[u8]) {
        self.count(residues, 1);
    }

    pub fn remove(&mut self, residues: &[u8]) {
        self.count(residues, u64::MAX);
    }

    /// Add `step` per letter: 1 to add, `u64::MAX` (wrapping) to remove.
    fn count(&mut self, residues: &[u8], step: u64) {
        for &b in residues {
            let b = b.to_ascii_uppercase();
            if matches!(b, b'N' | b'X') || !(b.is_ascii_alphabetic() || b == b'*') {
                continue;
            }
            self.letters = self.letters.wrapping_add(step);
            let class = match b {
                b'E' | b'F' | b'I' | b'L' | b'P' | b'Q' | b'J' | b'O' | b'Z' | b'*' => {
                    &mut self.protein_only
                }
                b'T' => &mut self.t,
                b'U' => &mut self.u,
                _ => continue,
            };
            *class = class.wrapping_add(step);
        }
    }

    /// No protein-only letters: DNA, or RNA when U appears without T (IUPAC
    /// ambiguity codes count as nucleotides). Protein-only letters above 1%
    /// of the counted letters: protein. Fewer: undetermined.
    pub fn alphabet(&self) -> Alphabet {
        if self.letters == 0 {
            Alphabet::Unknown
        } else if self.protein_only == 0 {
            if self.u > 0 && self.t == 0 {
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
        AlphabetCounts::of(residues).alphabet()
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
        // Exactly 1% is still undetermined: 1 E in 100 letters.
        let one_percent = with(b"ACGT", b"E", 99);
        assert_eq!(Alphabet::guess(&one_percent), Alphabet::Undetermined);
        assert_eq!(Alphabet::guess(b"MKLVEFIQ*"), Alphabet::Protein);
        assert_eq!(Alphabet::guess(b"--??NNXX"), Alphabet::Unknown);
    }

    #[test]
    fn counts_follow_edits() {
        let mut counts = super::AlphabetCounts::of(b"ACGTACGT");
        counts.add(b"MELQ");
        assert_eq!(counts.alphabet(), Alphabet::Protein);
        counts.remove(b"MELQ");
        assert_eq!(counts, super::AlphabetCounts::of(b"ACGTACGT"));
        counts.remove(b"TT");
        counts.add(b"UU");
        assert_eq!(counts.alphabet(), Alphabet::Rna);
    }

    #[test]
    fn a_document_has_an_alphabet_only_when_its_sequences_agree() {
        use Alphabet::{Dna, Mixed, Protein, Unknown};
        assert_eq!(Alphabet::common([Dna, Unknown, Dna]), Dna);
        assert_eq!(Alphabet::common([Dna, Protein]), Mixed);
        assert_eq!(Alphabet::common([]), Unknown);
    }
}
