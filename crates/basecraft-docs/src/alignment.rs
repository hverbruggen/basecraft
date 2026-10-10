//! An open alignment: rows of equal length, each a piece table over the
//! mapped file, so memory grows with the edits and not with the file.
//!
//! Every edit keeps the alignment rectangular. A row that grows makes the
//! alignment wider, and the other rows get trailing gaps; a row that shrinks
//! gets trailing gaps back. Inserting gaps into some rows uses up their
//! trailing gaps first, so nudging a row right doesn't widen the alignment
//! when there's room at its end.
//!
//! Each edit returns the [`Undo`] steps that reverse it exactly.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use basecraft_io::cache::IndexedFasta;

use crate::piece::{PieceTable, Residues};
use crate::sequence::FastaResidues;

pub const GAP: u8 = b'-';

pub fn is_gap(b: u8) -> bool {
    b == b'-' || b == b'.'
}

/// Half-open row ranges, `[(first, past_last)]`, in any order.
pub type RowRanges = [(u32, u32)];

#[derive(Clone)]
pub struct AlnRow {
    pub name: String,
    /// The header after the name, kept for saving.
    pub description: String,
    seq: PieceTable,
}

impl AlnRow {
    pub fn new(name: impl Into<String>, residues: Arc<dyn Residues>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            seq: PieceTable::new(residues),
        }
    }

    pub fn len(&self) -> u64 {
        self.seq.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seq.is_empty()
    }
}

/// One step that reverses part of an edit. An edit's steps are undone in
/// reverse order.
#[derive(Clone)]
pub enum Undo {
    /// Replace `len` residues at `pos` in `row` with `residues`.
    Splice {
        row: usize,
        pos: u64,
        len: u64,
        residues: Vec<u8>,
    },
    InsertRow {
        index: usize,
        row: Box<AlnRow>,
    },
    Rename {
        row: usize,
        name: String,
    },
    /// Put rows back in order: row `i` becomes the current row `order[i]`.
    Reorder {
        order: Vec<u32>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditError(pub String);

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditError {}

fn err<T>(msg: impl Into<String>) -> Result<T, EditError> {
    Err(EditError(msg.into()))
}

/// Consensus of one column: the most common residue and the fraction of
/// rows that have it; `-` and 0 for a column of gaps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Consensus {
    pub residue: u8,
    pub agreement: f32,
}

/// Columns per consensus cache entry.
const CONSENSUS_CHUNK: u64 = 1024;

#[derive(Default)]
struct ConsensusCache {
    version: u64,
    chunks: HashMap<u64, Arc<Vec<Consensus>>>,
}

pub struct AlignmentDoc {
    pub name: String,
    rows: Vec<AlnRow>,
    width: u64,
    version: u64,
    protein: bool,
    consensus: Mutex<ConsensusCache>,
}

impl AlignmentDoc {
    /// An alignment from rows. Rows shorter than the longest are padded with
    /// trailing gaps, as reading a ragged alignment file does.
    pub fn new(name: impl Into<String>, rows: Vec<AlnRow>) -> Self {
        let mut doc = Self {
            name: name.into(),
            rows,
            width: 0,
            version: 0,
            protein: false,
            consensus: Mutex::new(ConsensusCache::default()),
        };
        doc.square(&mut Vec::new());
        doc.protein = doc.detect_protein();
        doc
    }

    /// Every record of an indexed FASTA file, as rows.
    pub fn from_fasta(name: impl Into<String>, file: &Arc<IndexedFasta>) -> Self {
        let rows = file
            .index
            .records
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let header = String::from_utf8_lossy(r.header(file.bytes())).into_owned();
                let description = header
                    .split_once(char::is_whitespace)
                    .map_or("", |(_, d)| d.trim())
                    .to_string();
                AlnRow {
                    name: r.name.clone(),
                    description,
                    seq: PieceTable::new(Arc::new(FastaResidues {
                        file: Arc::clone(file),
                        record: i,
                    })),
                }
            })
            .collect();
        Self::new(name, rows)
    }

    pub fn width(&self) -> u64 {
        self.width
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, i: usize) -> &AlnRow {
        &self.rows[i]
    }

    pub fn rows(&self) -> impl Iterator<Item = &AlnRow> {
        self.rows.iter()
    }

    /// Increases with every edit.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Whether the residues look like amino acids rather than nucleotides.
    pub fn is_protein(&self) -> bool {
        self.protein
    }

    pub fn read(&self, row: usize, start: u64, end: u64) -> Vec<u8> {
        self.rows[row].seq.read(start, end)
    }

    pub fn read_into(&self, row: usize, start: u64, end: u64, out: &mut Vec<u8>) {
        self.rows[row].seq.read_into(start, end, out);
    }

    /// Bytes held by edits, over all rows.
    pub fn edit_bytes(&self) -> usize {
        self.rows.iter().map(|r| r.seq.edit_bytes()).sum()
    }

    fn detect_protein(&self) -> bool {
        let (mut nucleotide, mut letters) = (0u32, 0u32);
        let mut buf = Vec::new();
        for i in 0..self.rows.len().min(20) {
            buf.clear();
            self.read_into(i, 0, self.width.min(2000), &mut buf);
            for &b in &buf {
                if b.is_ascii_alphabetic() {
                    letters += 1;
                    if b"ACGTUNacgtun".contains(&b) {
                        nucleotide += 1;
                    }
                }
            }
        }
        letters > 0 && f64::from(nucleotide) < 0.9 * f64::from(letters)
    }

    /// Rows in `ranges`, ascending, without duplicates; checked against the
    /// row count.
    pub fn expand(&self, ranges: &RowRanges) -> Result<Vec<usize>, EditError> {
        let mut rows = Vec::new();
        for &(a, b) in ranges {
            if a >= b || b as usize > self.rows.len() {
                return err(format!("rows {a}..{b} outside 0..{}", self.rows.len()));
            }
            rows.extend(a as usize..b as usize);
        }
        rows.sort_unstable();
        rows.dedup();
        if rows.is_empty() {
            return err("no rows given");
        }
        Ok(rows)
    }

    // Primitive steps. Each records its own undo step.

    fn splice(&mut self, row: usize, start: u64, end: u64, residues: &[u8], undo: &mut Vec<Undo>) {
        let seq = &mut self.rows[row].seq;
        let removed = seq.delete(start, end);
        seq.insert(start, residues);
        undo.push(Undo::Splice {
            row,
            pos: start,
            len: residues.len() as u64,
            residues: removed,
        });
    }

    /// Pad rows to the longest with trailing gaps, and set the width.
    fn square(&mut self, undo: &mut Vec<Undo>) {
        let width = self.rows.iter().map(AlnRow::len).max().unwrap_or(0);
        for i in 0..self.rows.len() {
            let len = self.rows[i].len();
            if len < width {
                self.splice(i, len, len, &gaps(width - len), undo);
            }
        }
        self.width = width;
    }

    fn edited(&mut self) {
        self.version += 1;
    }

    /// Trailing gaps of a row, counted back from its end.
    fn trailing_gaps(&self, row: usize, limit: u64) -> u64 {
        let len = self.rows[row].len();
        let mut n = 0;
        let mut buf = Vec::new();
        while n < limit && n < len {
            let take = (limit - n).min(4096).min(len - n);
            buf.clear();
            self.read_into(row, len - n - take, len - n, &mut buf);
            let run = buf.iter().rev().take_while(|&&b| is_gap(b)).count() as u64;
            n += run;
            if run < take {
                break;
            }
        }
        n.min(limit)
    }

    fn all_gaps(&self, row: usize, start: u64, end: u64) -> bool {
        self.read(row, start, end).iter().all(|&b| is_gap(b))
    }

    // Edits.

    /// Replace `start..end` of one row with `residues`. Typing over a residue
    /// is a replacement of the same length.
    pub fn replace(
        &mut self,
        row: usize,
        start: u64,
        end: u64,
        residues: &[u8],
    ) -> Result<Vec<Undo>, EditError> {
        if row >= self.rows.len() {
            return err(format!("no row {row}"));
        }
        if start > end || end > self.width {
            return err(format!("columns {start}..{end} outside 0..{}", self.width));
        }
        let mut undo = Vec::new();
        self.splice(row, start, end, residues, &mut undo);
        self.square(&mut undo);
        self.edited();
        Ok(undo)
    }

    /// Insert `count` gaps before column `col` in `rows`. When only some rows
    /// get gaps, each uses up its trailing gaps first; when every row does,
    /// it's a new gap column and the alignment widens.
    pub fn insert_gaps(
        &mut self,
        rows: &RowRanges,
        col: u64,
        count: u64,
    ) -> Result<Vec<Undo>, EditError> {
        let rows = self.expand(rows)?;
        if col > self.width {
            return err(format!("column {col} outside 0..={}", self.width));
        }
        let every_row = rows.len() == self.rows.len();
        let mut undo = Vec::new();
        for r in rows {
            if !every_row {
                let len = self.rows[r].len();
                let used = self.trailing_gaps(r, count.min(len - col));
                self.splice(r, len - used, len, &[], &mut undo);
            }
            self.splice(r, col, col, &gaps(count), &mut undo);
        }
        self.square(&mut undo);
        self.edited();
        Ok(undo)
    }

    /// Delete columns `start..end` of `rows`; what follows moves left. Deleting
    /// from every row removes the columns.
    pub fn delete(
        &mut self,
        rows: &RowRanges,
        start: u64,
        end: u64,
    ) -> Result<Vec<Undo>, EditError> {
        let rows = self.expand(rows)?;
        if start >= end || end > self.width {
            return err(format!("columns {start}..{end} outside 0..{}", self.width));
        }
        let mut undo = Vec::new();
        for r in rows {
            self.splice(r, start, end, &[], &mut undo);
        }
        self.square(&mut undo);
        self.edited();
        Ok(undo)
    }

    /// Slide the block `rows` × `start..end` by `by` columns, into gaps next
    /// to it. The gaps it moves into end up on its other side.
    pub fn shift(
        &mut self,
        rows: &RowRanges,
        start: u64,
        end: u64,
        by: i64,
    ) -> Result<Vec<Undo>, EditError> {
        let rows = self.expand(rows)?;
        if start >= end || end > self.width {
            return err(format!("columns {start}..{end} outside 0..{}", self.width));
        }
        let k = by.unsigned_abs();
        if by < 0 && k > start || by > 0 && end + k > self.width {
            return err("the block would move past the end of the alignment");
        }
        for &r in &rows {
            let (a, b) = if by < 0 {
                (start - k, start)
            } else {
                (end, end + k)
            };
            if !self.all_gaps(r, a, b) {
                return err(format!(
                    "{} has residues in the way; a block only slides into gaps",
                    self.rows[r].name
                ));
            }
        }
        let mut undo = Vec::new();
        if k == 0 {
            self.edited();
            return Ok(undo);
        }
        for r in rows {
            if by < 0 {
                self.splice(r, start - k, start, &[], &mut undo);
                self.splice(r, end - k, end - k, &gaps(k), &mut undo);
            } else {
                self.splice(r, end, end + k, &[], &mut undo);
                self.splice(r, start, start, &gaps(k), &mut undo);
            }
        }
        self.edited();
        Ok(undo)
    }

    /// Delete every column that is a gap in every row. Returns how many went.
    // Column indexes fit in usize: `all_gap` holds one flag per column.
    #[allow(clippy::cast_possible_truncation)]
    pub fn remove_gap_columns(&mut self) -> (Vec<Undo>, u64) {
        let width = self.width;
        let mut all_gap = vec![true; usize::try_from(width).unwrap_or(0)];
        let mut buf = Vec::new();
        let chunk = 1 << 16;
        for r in 0..self.rows.len() {
            let mut pos = 0;
            while pos < width {
                let end = (pos + chunk).min(width);
                buf.clear();
                self.read_into(r, pos, end, &mut buf);
                for (i, &b) in buf.iter().enumerate() {
                    if !is_gap(b) {
                        all_gap[pos as usize + i] = false;
                    }
                }
                pos = end;
            }
        }
        // Runs of gap columns, deleted right to left so positions hold.
        let mut runs = Vec::new();
        let mut i = 0;
        while i < all_gap.len() {
            if all_gap[i] {
                let s = i;
                while i < all_gap.len() && all_gap[i] {
                    i += 1;
                }
                runs.push((s as u64, i as u64));
            } else {
                i += 1;
            }
        }
        let removed = runs.iter().map(|(a, b)| b - a).sum();
        let mut undo = Vec::new();
        if removed == 0 {
            return (undo, 0);
        }
        for r in 0..self.rows.len() {
            for &(a, b) in runs.iter().rev() {
                self.splice(r, a, b, &[], &mut undo);
            }
        }
        self.square(&mut undo);
        self.edited();
        (undo, removed)
    }

    /// Move `rows`, keeping their order, so the first of them lands at index
    /// `to` among the rows that remain.
    pub fn move_rows(&mut self, rows: &RowRanges, to: u32) -> Result<Vec<Undo>, EditError> {
        let moving = self.expand(rows)?;
        let rest: Vec<usize> = (0..self.rows.len())
            .filter(|i| moving.binary_search(i).is_err())
            .collect();
        let to = to as usize;
        if to > rest.len() {
            return err(format!("target {to} outside 0..={}", rest.len()));
        }
        let mut order: Vec<usize> = rest[..to].to_vec();
        order.extend(&moving);
        order.extend(&rest[to..]);
        let mut undo = Vec::new();
        self.permute(&order, &mut undo);
        self.edited();
        Ok(undo)
    }

    /// Reorder so that row `i` becomes the current row `order[i]`.
    fn permute(&mut self, order: &[usize], undo: &mut Vec<Undo>) {
        let mut old: Vec<Option<AlnRow>> = std::mem::take(&mut self.rows)
            .into_iter()
            .map(Some)
            .collect();
        let mut inverse = vec![0u32; order.len()];
        for (new, &from) in order.iter().enumerate() {
            inverse[from] = u32::try_from(new).expect("fewer than 2^32 rows");
            self.rows
                .push(old[from].take().expect("each row moves once"));
        }
        undo.push(Undo::Reorder { order: inverse });
    }

    pub fn rename(&mut self, row: usize, name: &str) -> Result<Vec<Undo>, EditError> {
        if row >= self.rows.len() {
            return err(format!("no row {row}"));
        }
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return err("a row name can't be empty or contain spaces");
        }
        let old = std::mem::replace(&mut self.rows[row].name, name.to_string());
        self.edited();
        Ok(vec![Undo::Rename { row, name: old }])
    }

    pub fn remove_rows(&mut self, rows: &RowRanges) -> Result<Vec<Undo>, EditError> {
        let rows = self.expand(rows)?;
        if rows.len() == self.rows.len() {
            return err("an alignment needs at least one row");
        }
        let mut undo = Vec::new();
        for &r in rows.iter().rev() {
            let row = self.rows.remove(r);
            undo.push(Undo::InsertRow {
                index: r,
                row: Box::new(row),
            });
        }
        self.edited();
        Ok(undo)
    }

    /// Reverse an edit's steps.
    pub fn undo(&mut self, steps: &[Undo]) {
        for step in steps.iter().rev() {
            match step {
                Undo::Splice {
                    row,
                    pos,
                    len,
                    residues,
                } => {
                    let seq = &mut self.rows[*row].seq;
                    seq.delete(*pos, pos + len);
                    seq.insert(*pos, residues);
                }
                Undo::InsertRow { index, row } => self.rows.insert(*index, (**row).clone()),
                Undo::Rename { row, name } => self.rows[*row].name.clone_from(name),
                Undo::Reorder { order } => {
                    let order: Vec<usize> = order.iter().map(|&i| i as usize).collect();
                    self.permute(&order, &mut Vec::new());
                }
            }
        }
        self.width = self.rows.iter().map(AlnRow::len).max().unwrap_or(0);
        self.edited();
    }

    // Consensus.

    /// Consensus of columns `start..end`, cached per version.
    pub fn consensus(&self, start: u64, end: u64) -> Vec<Consensus> {
        let end = end.min(self.width);
        let mut out = Vec::new();
        if start >= end {
            return out;
        }
        for chunk in start / CONSENSUS_CHUNK..=(end - 1) / CONSENSUS_CHUNK {
            let cols = self.consensus_chunk(chunk);
            let base = chunk * CONSENSUS_CHUNK;
            let a = start.max(base) - base;
            let b = end.min(base + cols.len() as u64) - base;
            #[allow(clippy::cast_possible_truncation)]
            out.extend_from_slice(&cols[a as usize..b as usize]);
        }
        out
    }

    /// One column's consensus.
    pub fn consensus_at(&self, col: u64) -> Option<Consensus> {
        (col < self.width).then(|| {
            let cols = self.consensus_chunk(col / CONSENSUS_CHUNK);
            #[allow(clippy::cast_possible_truncation)]
            cols[(col % CONSENSUS_CHUNK) as usize]
        })
    }

    fn consensus_chunk(&self, chunk: u64) -> Arc<Vec<Consensus>> {
        {
            let mut cache = self
                .consensus
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.version != self.version {
                cache.version = self.version;
                cache.chunks.clear();
            }
            if let Some(c) = cache.chunks.get(&chunk) {
                return Arc::clone(c);
            }
        }
        let start = chunk * CONSENSUS_CHUNK;
        let end = (start + CONSENSUS_CHUNK).min(self.width);
        #[allow(clippy::cast_possible_truncation)]
        let n = (end - start) as usize;
        // Counts per column: A..Z, then gaps and anything else.
        let mut counts = vec![[0u32; 28]; n];
        let mut buf = Vec::with_capacity(n);
        for r in 0..self.rows.len() {
            buf.clear();
            self.read_into(r, start, end, &mut buf);
            for (c, &b) in buf.iter().enumerate() {
                let slot = match b {
                    b'A'..=b'Z' => usize::from(b - b'A'),
                    b'a'..=b'z' => usize::from(b - b'a'),
                    b'-' | b'.' => 26,
                    _ => 27,
                };
                counts[c][slot] += 1;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let rows = self.rows.len().max(1) as f32;
        let cols: Vec<Consensus> = counts
            .iter()
            .map(|c| {
                let (best, &count) = c[..26]
                    .iter()
                    .enumerate()
                    .max_by_key(|&(i, &n)| (n, std::cmp::Reverse(i)))
                    .unwrap_or((0, &0));
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                if count == 0 {
                    Consensus {
                        residue: GAP,
                        agreement: 0.0,
                    }
                } else {
                    Consensus {
                        residue: b'A' + best as u8,
                        agreement: count as f32 / rows,
                    }
                }
            })
            .collect();
        let cols = Arc::new(cols);
        let mut cache = self
            .consensus
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.version == self.version {
            cache.chunks.insert(chunk, Arc::clone(&cols));
        }
        cols
    }
}

pub fn gaps(n: u64) -> Vec<u8> {
    vec![GAP; usize::try_from(n).expect("gap run fits in memory")]
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names
)]
mod tests {
    use super::*;

    fn aln(rows: &[&str]) -> AlignmentDoc {
        AlignmentDoc::new(
            "t",
            rows.iter()
                .enumerate()
                .map(|(i, r)| AlnRow::new(format!("r{i}"), Arc::new(r.as_bytes().to_vec())))
                .collect(),
        )
    }

    fn text(d: &AlignmentDoc) -> Vec<String> {
        (0..d.row_count())
            .map(|r| String::from_utf8(d.read(r, 0, d.width())).unwrap())
            .collect()
    }

    fn names(d: &AlignmentDoc) -> Vec<String> {
        d.rows().map(|r| r.name.clone()).collect()
    }

    #[test]
    fn ragged_rows_are_padded() {
        let d = aln(&["ACGT", "AC", "ACGTAC"]);
        assert_eq!(d.width(), 6);
        assert_eq!(text(&d), ["ACGT--", "AC----", "ACGTAC"]);
    }

    #[test]
    fn gaps_use_trailing_room_or_widen() {
        let mut d = aln(&["ACGT--", "ACGTAC"]);
        d.insert_gaps(&[(0, 1)], 1, 2).unwrap();
        assert_eq!(text(&d), ["A--CGT", "ACGTAC"]);
        d.insert_gaps(&[(0, 1)], 0, 1).unwrap();
        assert_eq!(text(&d), ["-A--CGT", "ACGTAC-"]);
        // Every row: a new column.
        d.insert_gaps(&[(0, 2)], 1, 1).unwrap();
        assert_eq!(text(&d), ["--A--CGT", "A-CGTAC-"]);
    }

    #[test]
    fn delete_pads_or_removes_columns() {
        let mut d = aln(&["ACGTAC", "TTGGCC"]);
        d.delete(&[(0, 1)], 1, 3).unwrap();
        assert_eq!(text(&d), ["ATAC--", "TTGGCC"]);
        d.delete(&[(0, 2)], 4, 6).unwrap();
        assert_eq!(text(&d), ["ATAC", "TTGG"]);
        assert_eq!(d.width(), 4);
    }

    #[test]
    fn shift_slides_into_gaps_only() {
        let mut d = aln(&["--ACG-", "T-ACG-"]);
        d.shift(&[(0, 1)], 2, 5, -2).unwrap();
        assert_eq!(text(&d), ["ACG---", "T-ACG-"]);
        assert!(d.shift(&[(1, 2)], 2, 5, -2).is_err());
        d.shift(&[(0, 2)], 2, 5, 1).unwrap();
        assert_eq!(text(&d), ["AC-G--", "T--ACG"]);
        assert!(d.shift(&[(1, 2)], 3, 6, 1).is_err());
    }

    #[test]
    fn rows_move_rename_and_go() {
        let mut d = aln(&["A", "C", "G", "T"]);
        d.move_rows(&[(2, 4)], 0).unwrap();
        assert_eq!(names(&d), ["r2", "r3", "r0", "r1"]);
        let u = d.rename(0, "x").unwrap();
        assert_eq!(d.row(0).name, "x");
        d.undo(&u);
        assert_eq!(d.row(0).name, "r2");
        let u = d.remove_rows(&[(1, 2), (3, 4)]).unwrap();
        assert_eq!(names(&d), ["r2", "r0"]);
        d.undo(&u);
        assert_eq!(names(&d), ["r2", "r3", "r0", "r1"]);
        assert!(d.remove_rows(&[(0, 4)]).is_err());
    }

    #[test]
    fn gap_columns_go() {
        let mut d = aln(&["A--C-G-", "T-.G-C-"]);
        let (u, n) = d.remove_gap_columns();
        assert_eq!(n, 4);
        assert_eq!(text(&d), ["ACG", "TGC"]);
        d.undo(&u);
        assert_eq!(text(&d), ["A--C-G-", "T-.G-C-"]);
    }

    #[test]
    fn consensus_counts_rows() {
        let d = aln(&["AC-", "AG-", "TG-", "AG-"]);
        let c = d.consensus(0, 3);
        assert_eq!(c[0].residue, b'A');
        assert!((c[0].agreement - 0.75).abs() < 1e-6);
        assert_eq!(c[1].residue, b'G');
        assert_eq!(c[2].residue, b'-');
        assert_eq!(d.consensus_at(1), Some(c[1]));
    }

    #[test]
    fn protein_detection() {
        assert!(!aln(&["ACGTNACGT-", "ACG-TACGT"]).is_protein());
        assert!(aln(&["MKLVWQERST", "MKLI-QERST"]).is_protein());
    }

    /// Random edits, undone in reverse, give back the original exactly.
    #[test]
    fn random_edits_undo_to_the_original() {
        let mut x = 0x2545_F491u64;
        let mut rnd = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n.max(1)
        };
        let rows: Vec<String> = (0..8)
            .map(|i| {
                (0..40)
                    .map(|j| {
                        if (i + j) % 5 == 0 {
                            '-'
                        } else {
                            "ACGT".as_bytes()[(i * 7 + j) % 4] as char
                        }
                    })
                    .collect()
            })
            .collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let mut d = aln(&refs);
        let original = (text(&d), names(&d));
        let mut undo = Vec::new();
        for _ in 0..400 {
            let n = d.row_count() as u32;
            let a = rnd(u64::from(n)) as u32;
            let b = a + 1 + rnd(u64::from(n - a)) as u32;
            let w = d.width();
            let s = rnd(w);
            let e = (s + 1 + rnd(6)).min(w);
            let r = match rnd(8) {
                0 => d.replace(a as usize, s, e, b"ACG"),
                1 => d.insert_gaps(&[(a, b)], s, 1 + rnd(3)),
                2 => d.delete(&[(a, b)], s, e),
                3 => d.shift(&[(a, b)], s, e, rnd(5) as i64 - 2),
                4 => d.move_rows(&[(a, b)], rnd(u64::from(n - (b - a)) + 1) as u32),
                5 => d.remove_rows(&[(a, a + 1)]),
                6 => d.rename(a as usize, &format!("n{}", rnd(100))),
                _ => Ok(d.remove_gap_columns().0),
            };
            if let Ok(u) = r {
                for i in 0..d.row_count() {
                    assert_eq!(d.row(i).len(), d.width(), "rows stay rectangular");
                }
                undo.push(u);
            }
        }
        for u in undo.iter().rev() {
            d.undo(u);
        }
        assert_eq!((text(&d), names(&d)), original);
    }

    /// Two gap insertions in a row, the second after the first's gaps, end
    /// the same as one insertion of both: the log merges them.
    #[test]
    fn consecutive_gap_insertions_merge() {
        let mut x = 0x9E37_79B9u64;
        let mut rnd = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n.max(1)
        };
        for _ in 0..300 {
            let rows: Vec<String> = (0..4)
                .map(|_| {
                    let mut row: String = (0..2 + rnd(8))
                        .map(|_| "AC-".as_bytes()[rnd(3) as usize] as char)
                        .collect();
                    row.extend((0..rnd(4)).map(|_| '-'));
                    row
                })
                .collect();
            let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
            let a = rnd(4) as u32;
            let range = [(a, a + 1 + rnd(u64::from(4 - a)) as u32)];
            let mut one = aln(&refs);
            let col = rnd(one.width() + 1);
            let (c1, c2) = (1 + rnd(3), 1 + rnd(3));
            let mut two = aln(&refs);
            two.insert_gaps(&range, col, c1).unwrap();
            two.insert_gaps(&range, col + c1, c2).unwrap();
            one.insert_gaps(&range, col, c1 + c2).unwrap();
            assert_eq!(text(&one), text(&two), "{refs:?} {range:?} {col} {c1} {c2}");
        }
    }

    /// Likewise for a block slid in steps.
    #[test]
    fn consecutive_shifts_merge() {
        let mut d = aln(&["---ACG---", "T--ACG--A"]);
        let mut e = aln(&["---ACG---", "T--ACG--A"]);
        d.shift(&[(0, 2)], 3, 6, -1).unwrap();
        d.shift(&[(0, 2)], 2, 5, -1).unwrap();
        d.shift(&[(0, 2)], 1, 4, 3).unwrap();
        e.shift(&[(0, 2)], 3, 6, 1).unwrap();
        assert_eq!(text(&d), text(&e));
    }
}
