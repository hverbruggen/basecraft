//! Typed, read-only Tracy draft evidence. A draft is never an accepted call set.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Versioned, explicit rules for automatic review triage, not error probabilities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewSettings {
    pub strong_score: i32,
    pub paired_score: i32,
    /// A secondary peak at or above this percentage is ambiguous.
    pub ambiguity_percent: u32,
    pub allow_weak_disagreement: bool,
    /// Minimum candidate / strongest peak ratio in a non-confident opposing read.
    pub weak_support_percent: u32,
    pub check_reference: bool,
    /// Zero means any observed canonical occurrence; denominator is A/C/G/T rows.
    pub reference_percent: u32,
    pub reference_min_rows: u32,
}
impl Default for ReviewSettings {
    fn default() -> Self {
        Self {
            strong_score: 30,
            paired_score: 20,
            ambiguity_percent: 35,
            allow_weak_disagreement: true,
            weak_support_percent: 80,
            check_reference: true,
            reference_percent: 0,
            reference_min_rows: 1,
        }
    }
}
impl ReviewSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(0..=100).contains(&self.strong_score)
            || !(0..=self.strong_score).contains(&self.paired_score)
            || !(1..=100).contains(&self.ambiguity_percent)
            || !(1..=100).contains(&self.weak_support_percent)
            || self.reference_percent > 100
            || self.reference_min_rows == 0
            || self.reference_min_rows > 1_000_000
        {
            return Err(
                "Invalid review thresholds (paired score must not exceed strong score)".into(),
            );
        }
        Ok(())
    }
}

/// Frozen whole-alignment counts used by a recorded triage operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewReference {
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub counts: Vec<Option<[u32; 4]>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceCalls {
    pub sequence: String,
    pub positions: Vec<u32>,
    pub scores: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputRead {
    pub read: usize,
    pub path: PathBuf,
    pub sha256: String,
    pub orientation: Orientation,
    #[serde(default)]
    pub calls: Option<TraceCalls>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Forward,
    Reverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    Call,
    InternalGap,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadSupport {
    pub read: usize,
    pub base: char,
    pub kind: SupportKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftColumn {
    /// One-based column in the Tracy alignment, including gap columns.
    pub column: usize,
    pub consensus: char,
    pub support: Vec<ReadSupport>,
    pub conflict: bool,
    pub review: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Overlap {
    pub reads: [usize; 2],
    pub comparable_bases: usize,
    pub identity: f64,
    pub accepted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SangerDraft {
    pub status: String,
    pub engine: String,
    pub version_output: String,
    pub engine_sha256: String,
    pub mode: String,
    #[serde(default)]
    pub signal_threshold: Option<u32>,
    pub inputs: Vec<InputRead>,
    pub consensus: String,
    pub consensus_length: usize,
    pub minimum_overlap: usize,
    pub minimum_identity: f64,
    pub overlaps: Vec<Overlap>,
    pub aligned_columns: usize,
    pub conflict_columns: usize,
    pub columns: Vec<DraftColumn>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl SangerDraft {
    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("Sanger evidence exceeds 32 MiB".into());
        }
        let draft: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        draft.validate()?;
        Ok(draft)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self
            .signal_threshold
            .is_some_and(|v| !(1..=90).contains(&v))
        {
            return Err("Invalid signal cutoff".into());
        }
        if self.status != "draft_requires_review"
            || !matches!(self.engine.as_str(), "Tracy" | "ABI")
        {
            return Err("Expected a review-only Sanger draft".into());
        }
        if !(1..=3).contains(&self.inputs.len()) || (self.engine == "ABI" && self.inputs.len() != 1)
        {
            return Err("One to three reads are required".into());
        }
        if self.mode
            != (if self.inputs.len() == 1 {
                if self.engine == "ABI" {
                    "single"
                } else {
                    "basecall"
                }
            } else if self.inputs.len() == 2 {
                "consensus"
            } else {
                "assemble"
            })
        {
            return Err("Draft mode does not match read count".into());
        }
        if self.inputs.iter().enumerate().any(|(i, read)| {
            read.read != i + 1
                || read.sha256.len() != 64
                || !read.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        }) {
            return Err("Invalid read identity or hash".into());
        }
        for read in &self.inputs {
            if let Some(calls) = &read.calls
                && (calls.sequence.is_empty()
                    || calls.sequence.len() != calls.positions.len()
                    || calls.sequence.len() != calls.scores.len()
                    || !calls
                        .sequence
                        .bytes()
                        .all(|b| b"ACGTNRYKMSWBDHV".contains(&b))
                    || calls.positions.windows(2).any(|p| p[0] >= p[1]))
            {
                return Err("Invalid Tracy signal coordinates".into());
            }
        }
        if self.aligned_columns == 0
            || self.aligned_columns > 100_000
            || self.consensus_length == 0
            || !self
                .consensus
                .bytes()
                .all(|base| b"ACGTNRYKMSWBDHV-".contains(&base))
        {
            return Err("Invalid draft consensus sequence".into());
        }
        if self.columns.len() != self.aligned_columns || self.columns.len() != self.consensus.len()
        {
            return Err("Consensus and evidence columns have different widths".into());
        }
        if self.consensus.chars().filter(|&base| base != '-').count() != self.consensus_length {
            return Err("Consensus length does not match evidence".into());
        }
        for (i, column) in self.columns.iter().enumerate() {
            if column.column != i + 1 || self.consensus.as_bytes()[i] != column.consensus as u8 {
                return Err("Evidence columns do not match consensus".into());
            }
            let mut seen = [false; 3];
            for support in &column.support {
                if support.read == 0
                    || support.read > self.inputs.len()
                    || std::mem::replace(&mut seen[support.read - 1], true)
                {
                    return Err("Invalid or duplicate read support".into());
                }
                if !support.base.is_ascii() || !b"ACGTNRYKMSWBDHV-".contains(&(support.base as u8))
                {
                    return Err("Invalid support base".into());
                }
                if (support.kind == SupportKind::InternalGap) != (support.base == '-') {
                    return Err("Gap support is inconsistent".into());
                }
            }
        }
        if self.conflict_columns != self.columns.iter().filter(|c| c.conflict).count() {
            return Err("Conflict count does not match evidence".into());
        }
        Ok(())
    }

    /// Orient a draft to the opposite strand while retaining the mapping to
    /// each immutable input trace. Review decisions are created afterwards.
    #[must_use]
    pub fn reverse_complemented(&self) -> Self {
        fn complement(base: char) -> char {
            match base {
                'A' => 'T',
                'T' => 'A',
                'C' => 'G',
                'G' => 'C',
                'R' => 'Y',
                'Y' => 'R',
                'K' => 'M',
                'M' => 'K',
                'B' => 'V',
                'V' => 'B',
                'D' => 'H',
                'H' => 'D',
                other => other,
            }
        }
        let mut result = self.clone();
        result.consensus = self.consensus.chars().rev().map(complement).collect();
        result.columns.reverse();
        for (i, column) in result.columns.iter_mut().enumerate() {
            column.column = i + 1;
            column.consensus = complement(column.consensus);
            for support in &mut column.support {
                support.base = complement(support.base);
            }
        }
        for read in &mut result.inputs {
            read.orientation = match read.orientation {
                Orientation::Forward => Orientation::Reverse,
                Orientation::Reverse => Orientation::Forward,
            };
        }
        result
    }

    /// The sequence of Tracy call events for one oriented read. Terminal
    /// noncoverage and internal gaps are excluded from this call sequence.
    pub fn proposed_read_calls(&self, read: usize) -> Vec<u8> {
        self.columns
            .iter()
            .flat_map(|column| {
                column
                    .support
                    .iter()
                    .filter(move |s| s.read == read && s.kind == SupportKind::Call)
                    .map(|s| s.base as u8)
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SangerPlacement {
    pub status: String,
    pub engine: String,
    pub version: String,
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub source_rows: usize,
    pub source_width: usize,
    pub fragment: String,
    pub map: Vec<Option<usize>>,
    pub aligned_probe: String,
    pub insertions: usize,
    pub parameters: Vec<String>,
}

impl SangerPlacement {
    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("Sanger placement exceeds 32 MiB".into());
        }
        let placement: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        placement.validate()?;
        Ok(placement)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.status != "candidate_requires_review"
            || self.engine != "MAFFT"
            || self.source_rows < 2
            || self.source_width == 0
            || self.source_width > 1_000_000
        {
            return Err("Invalid MAFFT placement provenance".into());
        }
        if self.source_sha256.len() != 64
            || !self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid source alignment hash".into());
        }
        if self.fragment.is_empty()
            || self.fragment.len() != self.map.len()
            || !self
                .fragment
                .bytes()
                .all(|b| b"ACGTNRYKMSWBDHV".contains(&b))
            || self.aligned_probe.len() != self.source_width
        {
            return Err("Fragment and placement coordinates disagree".into());
        }
        if self.map.iter().filter(|column| column.is_none()).count() != self.insertions {
            return Err("Insertion count does not match mapping".into());
        }
        let mapped: Vec<usize> = self.map.iter().flatten().copied().collect();
        if mapped.is_empty()
            || mapped.iter().any(|&column| column >= self.source_width)
            || mapped.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("Fragment mapping is outside or reverses the source alignment".into());
        }
        let mapped_bases: String = self
            .fragment
            .bytes()
            .zip(&self.map)
            .filter_map(|(base, column)| column.map(|_| char::from(base)))
            .collect();
        if self.aligned_probe.replace('-', "") != mapped_bases {
            return Err("Placed fragment differs from coordinate map".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mafft_placement_preserves_insertion_anchors() {
        let path = Path::new("/tmp/basecraft-sanger-place-test.json");
        if !path.exists() {
            return;
        }
        let placement = SangerPlacement::read(path).unwrap();
        assert_eq!(placement.insertions, 3);
        assert_eq!(placement.map.len(), placement.fragment.len());
        let mut invalid = placement.clone();
        invalid.map[0] = Some(invalid.source_width);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn malformed_draft_cannot_be_treated_as_reviewable_evidence() {
        let raw = serde_json::json!({
            "status":"draft_requires_review", "engine":"Tracy", "version_output":"test",
            "engine_sha256":"a".repeat(64), "mode":"consensus",
            "inputs":[
                {"read":1,"path":"a.ab1","sha256":"a".repeat(64),"orientation":"forward"},
                {"read":2,"path":"b.ab1","sha256":"b".repeat(64),"orientation":"reverse"}
            ],
            "consensus":"AC", "consensus_length":2,
            "minimum_overlap":1, "minimum_identity":0.8,
            "overlaps":[], "aligned_columns":2, "conflict_columns":0,
            "columns":[
                {"column":1,"consensus":"A","support":[{"read":1,"base":"A","kind":"call"}],"conflict":false,"review":true},
                {"column":2,"consensus":"C","support":[{"read":2,"base":"C","kind":"call"}],"conflict":false,"review":true}
            ]
        });
        let mut draft: SangerDraft = serde_json::from_value(raw).unwrap();
        assert!(draft.validate().is_ok());
        assert_eq!(draft.proposed_read_calls(2), b"C");
        let reversed = draft.reverse_complemented();
        reversed.validate().unwrap();
        assert_eq!(reversed.consensus, "GT");
        assert_eq!(reversed.inputs[0].orientation, Orientation::Reverse);
        assert_eq!(reversed.proposed_read_calls(2), b"G");
        assert_eq!(reversed.reverse_complemented(), draft);
        draft.columns[1].column = 1;
        assert!(draft.validate().is_err());
    }
}
