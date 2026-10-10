//! Coordinate maps: how each edit moves positions. The same map moves
//! annotations, keeps selections stable, and lets a plugin's result computed
//! on an older version land in the right place.

use basecraft_protocol::{Location, Segment};
use serde::{Deserialize, Serialize};

/// One edit's effect on coordinates: at `pos`, `deleted` residues were
/// removed and `inserted` residues put in their place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub pos: u64,
    pub deleted: u64,
    pub inserted: u64,
}

/// Which side a position sticks to when residues are inserted exactly at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    /// Stays before the insertion.
    Left,
    /// Moves past the insertion.
    Right,
}

impl Change {
    pub fn map(&self, p: u64, bias: Bias) -> u64 {
        if p < self.pos {
            return p;
        }
        if p == self.pos || p < self.pos + self.deleted {
            // At the edit point, or inside the replaced range.
            return match bias {
                Bias::Left => self.pos,
                Bias::Right => self.pos + self.inserted,
            };
        }
        p - self.deleted + self.inserted
    }

    pub fn is_insertion(&self) -> bool {
        self.deleted == 0
    }
}

/// Map a list of changes, applied in order.
pub fn map_through(changes: &[Change], mut p: u64, bias: Bias) -> u64 {
    for c in changes {
        p = c.map(p, bias);
    }
    p
}

/// Map a feature location through a change. Insertions strictly inside a
/// segment extend it; insertions at its edges fall outside. Segments that
/// lose all their residues are dropped. Returns `None` if nothing is left.
pub fn map_location(loc: &Location, change: &Change) -> Option<Location> {
    let mut segments = Vec::with_capacity(loc.segments.len());
    for s in &loc.segments {
        if s.is_empty() {
            // A site between bases moves like a cursor.
            let p = change.map(s.start, Bias::Left);
            segments.push(Segment {
                start: p,
                end: p,
                ..s.clone()
            });
            continue;
        }
        // Pure insertions at an edge fall outside the segment. For
        // replacements, the replaced residues that were part of the segment
        // stay part of it, so a substitution at the first base keeps it.
        let (start, end) = if change.is_insertion() {
            (
                change.map(s.start, Bias::Right),
                change.map(s.end, Bias::Left),
            )
        } else {
            (
                change.map(s.start, Bias::Left),
                change.map(s.end, Bias::Right),
            )
        };
        // A change that covers the segment and reaches beyond it removes it.
        let change_end = change.pos + change.deleted;
        let swallowed = change.pos <= s.start
            && change_end >= s.end
            && (change.pos < s.start || change_end > s.end);
        if end > start && !swallowed {
            segments.push(Segment {
                start,
                end,
                ..s.clone()
            });
        }
    }
    (!segments.is_empty()).then_some(Location {
        segments,
        unordered: loc.unordered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_protocol::Strand;

    fn ins(pos: u64, n: u64) -> Change {
        Change {
            pos,
            deleted: 0,
            inserted: n,
        }
    }

    fn del(pos: u64, n: u64) -> Change {
        Change {
            pos,
            deleted: n,
            inserted: 0,
        }
    }

    #[test]
    fn positions() {
        assert_eq!(ins(10, 3).map(5, Bias::Left), 5);
        assert_eq!(ins(10, 3).map(10, Bias::Left), 10);
        assert_eq!(ins(10, 3).map(10, Bias::Right), 13);
        assert_eq!(ins(10, 3).map(11, Bias::Left), 14);
        assert_eq!(del(10, 5).map(12, Bias::Left), 10);
        assert_eq!(del(10, 5).map(15, Bias::Left), 10);
        assert_eq!(del(10, 5).map(20, Bias::Left), 15);
        let rep = Change {
            pos: 10,
            deleted: 5,
            inserted: 2,
        };
        assert_eq!(rep.map(12, Bias::Right), 12);
        assert_eq!(rep.map(20, Bias::Left), 17);
    }

    #[allow(clippy::ref_option)]
    fn seg(loc: &Option<Location>) -> Vec<(u64, u64)> {
        loc.as_ref()
            .map(|l| l.segments.iter().map(|s| (s.start, s.end)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn features_move_grow_shrink_and_vanish() {
        let gene = Location::simple(100, 200, Strand::Forward);
        // Upstream insertion shifts it.
        assert_eq!(seg(&map_location(&gene, &ins(50, 10))), vec![(110, 210)]);
        // Insertion at the start edge falls outside.
        assert_eq!(seg(&map_location(&gene, &ins(100, 10))), vec![(110, 210)]);
        // Insertion inside grows it.
        assert_eq!(seg(&map_location(&gene, &ins(150, 10))), vec![(100, 210)]);
        // Insertion at the end edge falls outside.
        assert_eq!(seg(&map_location(&gene, &ins(200, 10))), vec![(100, 200)]);
        // Deletion overlapping the start trims it.
        assert_eq!(seg(&map_location(&gene, &del(90, 20))), vec![(90, 180)]);
        // Deletion covering it removes it.
        assert_eq!(seg(&map_location(&gene, &del(90, 200))), vec![]);
        // Downstream edits leave it alone.
        assert_eq!(seg(&map_location(&gene, &del(300, 20))), vec![(100, 200)]);
    }

    #[test]
    fn substitutions_keep_features_intact() {
        let gene = Location::simple(100, 200, Strand::Forward);
        let sub = |pos| Change {
            pos,
            deleted: 1,
            inserted: 1,
        };
        assert_eq!(seg(&map_location(&gene, &sub(100))), vec![(100, 200)]);
        assert_eq!(seg(&map_location(&gene, &sub(199))), vec![(100, 200)]);
        let snp = Location::simple(150, 151, Strand::Forward);
        assert_eq!(seg(&map_location(&snp, &sub(150))), vec![(150, 151)]);
    }

    #[test]
    fn multi_segment_loses_one_exon() {
        let mut loc = Location::simple(100, 200, Strand::Forward);
        loc.segments.push(Segment::new(300, 400, Strand::Forward));
        let mapped = map_location(&loc, &del(290, 120));
        assert_eq!(seg(&mapped), vec![(100, 200)]);
    }
}
