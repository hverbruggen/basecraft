//! Piece table over an immutable base sequence and an append buffer.
//! Unedited regions point into the base, which may be a memory-mapped file,
//! so memory grows with the size of the edits, not of the sequence.

use std::sync::Arc;

/// An immutable residue source: a mapped FASTA record or an owned buffer.
pub trait Residues: Send + Sync {
    fn len(&self) -> u64;
    /// Append residues `start..end` to `out`.
    fn read_into(&self, start: u64, end: u64, out: &mut Vec<u8>);

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Residues for Vec<u8> {
    fn len(&self) -> u64 {
        self.len() as u64
    }

    fn read_into(&self, start: u64, end: u64, out: &mut Vec<u8>) {
        #[allow(clippy::cast_possible_truncation)]
        out.extend_from_slice(&self[start as usize..end as usize]);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    Base,
    Add,
}

#[derive(Clone, Copy, Debug)]
struct Piece {
    src: Src,
    start: u64,
    len: u64,
}

#[derive(Clone)]
pub struct PieceTable {
    base: Arc<dyn Residues>,
    add: Vec<u8>,
    pieces: Vec<Piece>,
    /// `ends[i]` is the document offset just past piece `i`.
    ends: Vec<u64>,
}

impl PieceTable {
    pub fn new(base: Arc<dyn Residues>) -> Self {
        let len = base.len();
        let pieces = if len == 0 {
            Vec::new()
        } else {
            vec![Piece {
                src: Src::Base,
                start: 0,
                len,
            }]
        };
        let mut t = Self {
            base,
            add: Vec::new(),
            pieces,
            ends: Vec::new(),
        };
        t.reindex();
        t
    }

    pub fn len(&self) -> u64 {
        self.ends.last().copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of pieces; 1 for an unedited sequence.
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// Bytes held by edits.
    pub fn edit_bytes(&self) -> usize {
        self.add.len() + self.pieces.len() * std::mem::size_of::<Piece>()
    }

    fn reindex(&mut self) {
        self.ends.clear();
        let mut acc = 0;
        for p in &self.pieces {
            acc += p.len;
            self.ends.push(acc);
        }
    }

    /// Append residues `start..end` to `out`.
    pub fn read_into(&self, start: u64, end: u64, out: &mut Vec<u8>) {
        let end = end.min(self.len());
        if start >= end {
            return;
        }
        let mut i = self.ends.partition_point(|&e| e <= start);
        let mut pos = start;
        while pos < end {
            let p = self.pieces[i];
            let piece_start = self.ends[i] - p.len;
            let from = p.start + (pos - piece_start);
            let take = (self.ends[i].min(end)) - pos;
            match p.src {
                Src::Base => self.base.read_into(from, from + take, out),
                #[allow(clippy::cast_possible_truncation)]
                Src::Add => out.extend_from_slice(&self.add[from as usize..(from + take) as usize]),
            }
            pos += take;
            i += 1;
        }
    }

    pub fn read(&self, start: u64, end: u64) -> Vec<u8> {
        let mut out = Vec::new();
        self.read_into(start, end, &mut out);
        out
    }

    /// Split so that a piece boundary falls at `pos`; returns the index of
    /// the first piece at or after `pos`.
    fn split_at(&mut self, pos: u64) -> usize {
        let i = self.ends.partition_point(|&e| e <= pos);
        if i == self.pieces.len() {
            return i;
        }
        let piece_start = self.ends[i] - self.pieces[i].len;
        if piece_start == pos {
            return i;
        }
        let p = self.pieces[i];
        let left = pos - piece_start;
        self.pieces[i].len = left;
        self.pieces.insert(
            i + 1,
            Piece {
                src: p.src,
                start: p.start + left,
                len: p.len - left,
            },
        );
        self.reindex();
        i + 1
    }

    pub fn insert(&mut self, pos: u64, residues: &[u8]) {
        assert!(pos <= self.len(), "insert at {pos} past end {}", self.len());
        if residues.is_empty() {
            return;
        }
        let start = self.add.len() as u64;
        self.add.extend_from_slice(residues);
        let len = residues.len() as u64;
        let i = self.split_at(pos);
        // Typing appends to the previous piece when it's the tail of `add`.
        if i > 0 {
            let prev = &mut self.pieces[i - 1];
            if prev.src == Src::Add && prev.start + prev.len == start {
                prev.len += len;
                self.reindex();
                return;
            }
        }
        self.pieces.insert(
            i,
            Piece {
                src: Src::Add,
                start,
                len,
            },
        );
        self.reindex();
    }

    /// Delete `start..end` and return the removed residues.
    pub fn delete(&mut self, start: u64, end: u64) -> Vec<u8> {
        assert!(
            start <= end && end <= self.len(),
            "delete {start}..{end} of {}",
            self.len()
        );
        let removed = self.read(start, end);
        if start == end {
            return removed;
        }
        let a = self.split_at(start);
        let b = self.split_at(end);
        self.pieces.drain(a..b);
        self.reindex();
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(s: &str) -> PieceTable {
        PieceTable::new(Arc::new(s.as_bytes().to_vec()))
    }

    fn text(t: &PieceTable) -> String {
        String::from_utf8(t.read(0, t.len())).unwrap()
    }

    #[test]
    fn edits_against_a_model() {
        let mut t = table("ACGTACGTAC");
        let mut model = "ACGTACGTAC".to_string();
        let edits: &[(&str, u64, u64, &str)] = &[
            ("ins", 0, 0, "GG"),
            ("ins", 12, 0, "TT"),
            ("ins", 5, 0, "N"),
            ("del", 3, 7, ""),
            ("ins", 3, 0, "CCC"),
            ("ins", 6, 0, "A"),
            ("del", 0, 2, ""),
            ("del", 5, 12, ""),
        ];
        for &(op, a, b, s) in edits {
            #[allow(clippy::cast_possible_truncation)]
            let (ua, ub) = (a as usize, b as usize);
            if op == "ins" {
                t.insert(a, s.as_bytes());
                model.insert_str(ua, s);
            } else {
                let removed = t.delete(a, b);
                assert_eq!(removed, &model.as_bytes()[ua..ub]);
                model.replace_range(ua..ub, "");
            }
            assert_eq!(text(&t), model);
            for i in 0..=model.len() {
                assert_eq!(t.read(i as u64, model.len() as u64), &model.as_bytes()[i..]);
            }
        }
    }

    #[test]
    fn typing_coalesces_pieces() {
        let mut t = table("AAAA");
        for (i, b) in b"CGTCGT".iter().enumerate() {
            t.insert(2 + i as u64, &[*b]);
        }
        assert_eq!(text(&t), "AACGTCGTAA");
        assert_eq!(t.piece_count(), 3);
    }
}
