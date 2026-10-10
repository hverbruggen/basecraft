//! Sequence annotations.
//!
//! Coordinates are 0-based and half-open everywhere inside Basecraft.
//! Readers and writers convert from and to the 1-based inclusive
//! coordinates of GenBank and GFF3.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Strand {
    Forward,
    Reverse,
    Unstranded,
}

/// One contiguous piece of a feature's location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

    /// Set every segment to `strand`, keeping biological order: changing
    /// between forward and reverse reverses the segment list, so the 5'
    /// segment stays first.
    pub fn set_strand(&mut self, strand: Strand) {
        let was_reverse = self.strand() == Strand::Reverse;
        let now_reverse = strand == Strand::Reverse;
        let (p5, p3) = (self.partial_5(), self.partial_3());
        for s in &mut self.segments {
            s.partial_start = false;
            s.partial_end = false;
        }
        if was_reverse != now_reverse {
            self.segments.reverse();
        }
        for s in &mut self.segments {
            s.strand = strand;
        }
        self.set_partial(p5, p3);
    }

    /// The 5' end is incomplete: GenBank `<` on a forward feature, `>` on a
    /// reverse one.
    pub fn partial_5(&self) -> bool {
        match (self.segments.first(), self.strand()) {
            (Some(s), Strand::Reverse) => s.partial_end,
            (Some(s), _) => s.partial_start,
            (None, _) => false,
        }
    }

    /// The 3' end is incomplete.
    pub fn partial_3(&self) -> bool {
        match (self.segments.last(), self.strand()) {
            (Some(s), Strand::Reverse) => s.partial_start,
            (Some(s), _) => s.partial_end,
            (None, _) => false,
        }
    }

    /// Mark the 5' and 3' ends complete or incomplete.
    pub fn set_partial(&mut self, five: bool, three: bool) {
        let reverse = self.strand() == Strand::Reverse;
        let n = self.segments.len();
        if n == 0 {
            return;
        }
        if reverse {
            self.segments[0].partial_end = five;
            self.segments[n - 1].partial_start = three;
        } else {
            self.segments[0].partial_start = five;
            self.segments[n - 1].partial_end = three;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn two(strand: Strand) -> Location {
        let mut segments = vec![Segment::new(10, 20, strand), Segment::new(30, 40, strand)];
        if strand == Strand::Reverse {
            segments.reverse();
        }
        Location {
            segments,
            unordered: false,
        }
    }

    #[test]
    fn partial_ends_follow_the_strand() {
        let mut f = two(Strand::Forward);
        f.set_partial(true, false);
        assert!(f.segments[0].partial_start && f.partial_5() && !f.partial_3());
        let mut r = two(Strand::Reverse);
        r.set_partial(true, false);
        // The 5' end of a reverse feature is its highest coordinate.
        assert_eq!((r.segments[0].start, r.segments[0].partial_end), (30, true));
    }

    #[test]
    fn changing_strand_keeps_order_and_partial_ends() {
        let mut l = two(Strand::Forward);
        l.set_partial(true, false);
        l.set_strand(Strand::Reverse);
        assert_eq!(l.segments[0].start, 30);
        assert!(l.partial_5() && !l.partial_3());
        assert!(l.segments[0].partial_end);
        l.set_strand(Strand::Unstranded);
        assert_eq!(l.segments[0].start, 10);
        assert!(l.partial_5());
    }
}
