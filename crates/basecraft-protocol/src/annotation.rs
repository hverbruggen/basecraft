//! Sequence annotations.
//!
//! Coordinates are 0-based and half-open everywhere inside Basecraft.
//! Readers and writers convert from and to the 1-based inclusive
//! coordinates of GenBank and GFF3.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strand {
    Forward,
    Reverse,
    Unstranded,
}

/// One contiguous piece of a feature's location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub strand: Strand,
    /// The feature extends beyond `start` (GenBank `<`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub partial_start: bool,
    /// The feature extends beyond `end` (GenBank `>`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub partial_end: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

impl Segment {
    pub fn new(start: u64, end: u64, strand: Strand) -> Self {
        Self {
            start,
            end,
            strand,
            partial_start: false,
            partial_end: false,
        }
    }

    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// A feature location: segments in biological order, 5' to 3' of the
/// feature. A reverse-strand two-exon gene lists its downstream exon first.
/// Locations crossing the origin of a circular sequence are split into two
/// segments at the origin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub segments: Vec<Segment>,
    /// GenBank `order()`: the segments' order is unknown, as opposed to
    /// `join()`, where they are joined in the given order.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unordered: bool,
}

impl Location {
    pub fn simple(start: u64, end: u64, strand: Strand) -> Self {
        Self {
            segments: vec![Segment::new(start, end, strand)],
            unordered: false,
        }
    }

    /// Smallest and largest coordinate covered, ignoring origin wrap.
    pub fn bounds(&self) -> Option<(u64, u64)> {
        let start = self.segments.iter().map(|s| s.start).min()?;
        let end = self.segments.iter().map(|s| s.end).max()?;
        Some((start, end))
    }

    pub fn strand(&self) -> Strand {
        match self.segments.first() {
            Some(first) if self.segments.iter().all(|s| s.strand == first.strand) => first.strand,
            _ => Strand::Unstranded,
        }
    }

    /// Total number of bases covered.
    pub fn len(&self) -> u64 {
        self.segments.iter().map(Segment::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Qualifier {
    pub key: String,
    /// `None` for flag qualifiers like GenBank `/pseudo`.
    pub value: Option<String>,
}

impl Qualifier {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: Some(value.into()),
        }
    }
}

/// A feature: a type, a location and qualifiers. GFF3 attributes map to
/// qualifiers too, with `ID` and `Parent` kept as ordinary qualifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feature {
    pub kind: String,
    pub location: Location,
    pub qualifiers: Vec<Qualifier>,
}

impl Feature {
    pub fn qualifier(&self, key: &str) -> Option<&str> {
        self.qualifiers
            .iter()
            .find(|q| q.key == key)
            .and_then(|q| q.value.as_deref())
    }

    /// A display name: the first of the usual naming qualifiers.
    pub fn label(&self) -> Option<&str> {
        ["Name", "gene", "locus_tag", "product", "label", "ID"]
            .iter()
            .find_map(|k| self.qualifier(k))
    }
}
