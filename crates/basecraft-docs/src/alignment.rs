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

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use basecraft_io::fasta::IndexedFasta;
use basecraft_protocol::facts::{Alphabet, AlphabetCounts, Fact, FactSource};

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
    /// Explicit row extent, excluding only synthetic rectangular suffix padding.
    content_len: u64,
    /// Letter counts for the row's guessed alphabet, kept current by edits.
    counts: AlphabetCounts,
}

impl AlnRow {
    pub fn new(name: impl Into<String>, residues: Arc<dyn Residues>) -> Self {
        Self::with_description(name.into(), String::new(), residues)
    }

    fn with_description(name: String, description: String, residues: Arc<dyn Residues>) -> Self {
        let content_len = residues.len();
        let seq = PieceTable::new(residues);
        let counts = crate::sequence::count_letters(&seq);
        Self {
            name,
            description,
            seq,
            content_len,
            counts,
        }
    }

    /// The row's guessed alphabet (D12).
    pub fn alphabet(&self) -> Alphabet {
        self.counts.alphabet()
    }

    /// Replace `start..end` with `residues`, recounting the letters when
    /// the edit reaches into the counted prefix.
    fn splice(&mut self, start: u64, end: u64, residues: &[u8]) -> Vec<u8> {
        let removed = self.seq.delete(start, end);
        self.seq.insert(start, residues);
        if self.counts.changed_by_edit_at(start) {
            self.counts = crate::sequence::count_letters(&self.seq);
        }
        removed
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
        content_len: u64,
    },
    InsertRow {
        index: usize,
        row: Box<AlnRow>,
    },
    RemoveRow {
        index: usize,
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

/// Consensus of one column and its fraction of supporting rows.
/// DNA uses the smallest IUPAC set supported by at least half the nongap rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Consensus {
    pub residue: u8,
    pub agreement: f32,
}

/// Canonical bases represented by an IUPAC DNA symbol, in A/C/G/T bit order.
pub fn dna_mask(base: u8) -> u8 {
    match base.to_ascii_uppercase() {
        b'A' => 1,
        b'C' => 2,
        b'G' => 4,
        b'T' | b'U' => 8,
        b'M' => 3,
        b'R' => 5,
        b'S' => 6,
        b'V' => 7,
        b'W' => 9,
        b'Y' => 10,
        b'H' => 11,
        b'K' => 12,
        b'D' => 13,
        b'B' => 14,
        b'N' => 15,
        _ => 0,
    }
}

#[allow(clippy::cast_precision_loss)] // Consensus agreement is stored as f32 throughout the editor.
fn dna_consensus(counts: &[u32; 28], rows: u32) -> Consensus {
    let rows = rows.saturating_sub(counts[26]);
    if rows == 0 {
        return Consensus {
            residue: GAP,
            agreement: 0.0,
        };
    }
    let mut best = (u32::MAX, 0, 15u8);
    for mask in 1u8..=15 {
        let count: u32 = counts[..26]
            .iter()
            .enumerate()
            .filter_map(|(i, &count)| {
                let observed = dna_mask(b'A' + u8::try_from(i).unwrap());
                (observed != 0 && observed & mask == observed).then_some(count)
            })
            .sum();
        if u64::from(count) * 2 >= u64::from(rows)
            && (mask.count_ones(), std::cmp::Reverse(count), mask)
                < (best.0, std::cmp::Reverse(best.1), best.2)
        {
            best = (mask.count_ones(), count, mask);
        }
    }
    if best.0 != u32::MAX {
        return Consensus {
            residue: b"?ACMGRSVTWYHKDBN"[best.2 as usize],
            agreement: best.1 as f32 / rows.max(1) as f32,
        };
    }
    // Missing data cannot supply votes for an unobserved base. Gaps have
    // already been removed from the denominator, including terminal padding.
    let count: u32 = counts[..26]
        .iter()
        .enumerate()
        .filter(|(i, _)| dna_mask(b'A' + u8::try_from(*i).unwrap()) != 0)
        .map(|(_, n)| n)
        .sum();
    Consensus {
        residue: b'N',
        agreement: count as f32 / rows.max(1) as f32,
    }
}

/// Whole-alignment composition of one DNA column. Ambiguous symbols are kept
/// separate rather than distributed across A/C/G/T by an arbitrary prior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnStats {
    pub acgt: [u32; 4],
    pub gap: u32,
    pub missing: u32,
    pub ambiguous: u32,
    pub other: u32,
}

impl ColumnStats {
    pub fn informative(&self) -> u32 {
        self.acgt.iter().sum()
    }

    /// Shannon diversity among unambiguous observed bases, in bits.
    pub fn entropy_bits(&self) -> Option<f64> {
        let n = self.informative();
        if n == 0 {
            return None;
        }
        let n = f64::from(n);
        Some(
            self.acgt
                .iter()
                .filter(|&&count| count > 0)
                .map(|&count| {
                    let p = f64::from(count) / n;
                    -p * p.log2()
                })
                .sum(),
        )
    }
}

/// Columns per consensus cache entry.
const CONSENSUS_CHUNK: u64 = 1024;

#[derive(Default)]
struct ConsensusCache {
    version: u64,
    chunks: HashMap<u64, Arc<Vec<Consensus>>>,
}

#[derive(Default)]
struct StatsCache {
    version: u64,
    chunks: HashMap<u64, Arc<Vec<ColumnStats>>>,
}

pub struct AlignmentDoc {
    pub name: String,
    rows: Vec<AlnRow>,
    width: u64,
    /// Ragged input keeps its explicit row extents through editing and saving.
    /// Already rectangular alignments retain their established gap-edit semantics.
    preserve_unpadded_lengths: bool,
    version: u64,
    /// The alphabet the side file or the user states.
    stated: Option<Fact<Alphabet>>,
    /// The rows' common guess, cached per version.
    guessed: Mutex<Option<(u64, Alphabet)>>,
    consensus: Mutex<ConsensusCache>,
    stats: Mutex<StatsCache>,
}

/// Why something that needs the alphabet can't run (D12), for sequences and
/// their guesses: some can't be told apart, or they are of different kinds.
/// Smaller groups are named first, so a lone odd row is easy to find.
pub fn alphabet_refusal<'a>(
    sequences: impl IntoIterator<Item = (&'a str, Alphabet)>,
) -> Option<String> {
    let mut groups: BTreeMap<Alphabet, Vec<&str>> = BTreeMap::new();
    for (name, alphabet) in sequences {
        if alphabet != Alphabet::Unknown {
            groups.entry(alphabet).or_default().push(name);
        }
    }
    let names = |names: &[&str]| {
        let shown = names
            .iter()
            .take(10)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        match names.len() {
            0..=10 => shown,
            n => format!("{shown} and {} more", n - 10),
        }
    };
    if let Some(unclear) = groups.get(&Alphabet::Undetermined) {
        return Some(format!(
            "Basecraft can't tell whether these sequences are DNA, RNA or protein: {}. State the alphabet first.",
            names(unclear)
        ));
    }
    if groups.len() < 2 {
        return None;
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|(_, names)| names.len());
    let largest = groups.len() - 1;
    let kinds: Vec<String> = (groups.iter().enumerate())
        .map(|(i, (alphabet, list))| match i {
            i if i == largest => format!("{} {alphabet}", list.len()),
            _ => format!("{} {alphabet}: {}", list.len(), names(list)),
        })
        .collect();
    Some(format!(
        "The sequences are not all of one kind ({}). State the alphabet, or split them by kind.",
        kinds.join("; ")
    ))
}

impl AlignmentDoc {
    /// An alignment from rows. Rows shorter than the longest are padded with
    /// trailing gaps, as reading a ragged alignment file does.
    pub fn new(name: impl Into<String>, rows: Vec<AlnRow>) -> Self {
        let preserve_unpadded_lengths = rows
            .first()
            .is_some_and(|first| rows.iter().any(|row| row.content_len != first.content_len));
        let mut doc = Self {
            name: name.into(),
            rows,
            width: 0,
            preserve_unpadded_lengths,
            version: 0,
            stated: None,
            guessed: Mutex::new(None),
            consensus: Mutex::new(ConsensusCache::default()),
            stats: Mutex::new(StatsCache::default()),
        };
        doc.square(&mut Vec::new());
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
                let residues = FastaResidues {
                    file: Arc::clone(file),
                    record: i,
                };
                AlnRow::with_description(r.name.clone(), description, Arc::new(residues))
            })
            .collect();
        Self::new(name, rows)
    }

    /// Require equal explicit row lengths, excluding synthetic suffix padding.
    /// Equal lengths alone do not establish biological alignment. Run an aligner
    /// on ungapped input to create a new alignment from unequal-length rows.
    pub fn validate_aligned_input(&self) -> Result<(), EditError> {
        if (0..self.rows.len()).any(|row| self.saved_row_len(row) != self.saved_row_len(0)) {
            return err(
                "This operation requires aligned sequences of equal length. These sequences have different lengths; align them first.",
            );
        }
        Ok(())
    }

    /// Row extent to serialize. Literal and explicitly edited gaps are retained;
    /// only synthetic suffix padding of originally ragged input is omitted.
    pub fn saved_row_len(&self, row: usize) -> u64 {
        if self.preserve_unpadded_lengths {
            self.rows[row].content_len
        } else {
            self.rows[row].len()
        }
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

    /// The letters the rows are written in, and who said so: the side file
    /// or the user, else the rows' common guess (D12), mixed when they differ.
    pub fn alphabet(&self) -> Fact<Alphabet> {
        if let Some(stated) = &self.stated {
            return stated.clone();
        }
        let mut cache = self
            .guessed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let value = match *cache {
            Some((version, value)) if version == self.version => value,
            _ => {
                let value = Alphabet::common(self.rows.iter().map(AlnRow::alphabet));
                *cache = Some((self.version, value));
                value
            }
        };
        Alphabet::guessed(value)
    }

    /// What the side file or the user states, which the side file keeps.
    pub fn stated_alphabet(&self) -> Option<&Fact<Alphabet>> {
        self.stated.as_ref()
    }

    /// Use the alphabet a side file or the user states; `None` drops it.
    /// Consensus and colours follow it.
    pub fn set_alphabet(&mut self, stated: Option<Fact<Alphabet>>) {
        if stated != self.stated {
            self.stated = stated;
            self.edited();
        }
    }

    /// Whether the alphabet, stated or guessed, is protein.
    pub fn is_protein(&self) -> bool {
        self.alphabet().value == Alphabet::Protein
    }

    /// A guessed alphabet whose rows disagree: no consensus or composition.
    fn is_mixed(&self) -> bool {
        self.alphabet().value == Alphabet::Mixed
    }

    /// Each row's guessed alphabet (D12).
    pub fn row_alphabets(&self) -> Vec<Alphabet> {
        self.rows.iter().map(AlnRow::alphabet).collect()
    }

    /// Why something that needs the alphabet can't run yet (D12): rows
    /// basecraft can't tell apart, or rows of different kinds. None when the
    /// alphabet is stated or every row agrees.
    pub fn alphabet_refusal(&self) -> Option<String> {
        if self.alphabet().source != FactSource::Guessed {
            return None;
        }
        alphabet_refusal(
            self.rows
                .iter()
                .map(|row| (row.name.as_str(), row.alphabet())),
        )
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
        let row_data = &mut self.rows[row];
        let content_len = row_data.content_len;
        let removed = row_data.splice(start, end, residues);
        // Explicit edits extend/shorten the meaningful extent even when their
        // bytes are gaps. Do not inspect residue values to infer alignment.
        row_data.content_len = if start < content_len {
            content_len - (end.min(content_len) - start) + residues.len() as u64
        } else if residues.is_empty() {
            content_len
        } else {
            start + residues.len() as u64
        };
        undo.push(Undo::Splice {
            row,
            pos: start,
            len: residues.len() as u64,
            residues: removed,
            content_len,
        });
    }

    /// Pad rows to the longest with trailing gaps, and set the width.
    fn square(&mut self, undo: &mut Vec<Undo>) {
        let width = self.rows.iter().map(AlnRow::len).max().unwrap_or(0);
        for i in 0..self.rows.len() {
            let len = self.rows[i].len();
            if len < width {
                let content_len = self.rows[i].content_len;
                self.splice(i, len, len, &gaps(width - len), undo);
                if self.preserve_unpadded_lengths {
                    self.rows[i].content_len = content_len;
                }
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

    /// Conflicting identifiers in input order, reported once each.
    pub fn duplicate_sequence_names(&self, rows: &[(String, String)]) -> Vec<String> {
        let mut names: std::collections::HashSet<&str> =
            self.rows.iter().map(|r| r.name.as_str()).collect();
        let mut reported = std::collections::HashSet::new();
        rows.iter()
            .filter(|(name, _)| !names.insert(name.as_str()) && reported.insert(name.as_str()))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Validate additions against this alignment's molecule alphabet.
    pub fn validate_additions(&self, rows: &[(String, String)]) -> Result<(), EditError> {
        // Rows that are not of one known kind take either set: the user may
        // be adding rows to settle exactly that.
        let (alphabet, kind): (&[u8], &str) = match self.alphabet().value {
            Alphabet::Dna | Alphabet::Rna => (b"ACGTUNRYKMSWBDHV-", "nucleotide"),
            Alphabet::Protein => (b"ACDEFGHIKLMNPQRSTVWYBXZJUO-*", "protein"),
            _ => (b"ACDEFGHIKLMNPQRSTVWYBXZJUO-*", "nucleotide or protein"),
        };
        for (name, sequence) in rows {
            if !sequence
                .bytes()
                .all(|b| alphabet.contains(&b.to_ascii_uppercase()))
            {
                return err(format!(
                    "Sequence {name} contains symbols outside this alignment's {kind} IUPAC alphabet"
                ));
            }
        }
        Ok(())
    }

    /// Append rows as one reversible edit. A tool may expand reference gaps,
    /// but must preserve every existing residue and row identity.
    pub fn add_sequences(
        &mut self,
        rows: &[(String, String)],
        reference: Option<&[String]>,
    ) -> Result<Vec<Undo>, EditError> {
        self.validate_additions(rows)?;
        let duplicates = self.duplicate_sequence_names(rows);
        if !duplicates.is_empty() {
            return err(format!(
                "Sequence IDs already used:\n{}",
                duplicates.join("\n")
            ));
        }
        let mut names: std::collections::HashSet<&str> =
            self.rows.iter().map(|r| r.name.as_str()).collect();
        if rows.is_empty()
            || rows.iter().any(|(name, seq)| {
                name.is_empty()
                    || name.chars().any(char::is_whitespace)
                    || seq.is_empty()
                    || !names.insert(name)
            })
        {
            return err("New sequences need nonempty sequences and unique names without spaces");
        }
        if let Some(reference) = reference {
            let width = reference.first().map_or(0, String::len);
            if reference.len() != self.rows.len()
                || width == 0
                || reference.iter().any(|s| s.len() != width)
                || rows.iter().any(|(_, s)| s.len() != width)
            {
                return err("Added alignment is not rectangular");
            }
            for (i, seq) in reference.iter().enumerate() {
                let before = self.read(i, 0, self.width);
                if !before
                    .iter()
                    .copied()
                    .filter(|b| !is_gap(*b))
                    .eq(seq.bytes().filter(|b| !is_gap(*b)))
                {
                    return err("Addition changed reference residues");
                }
            }
        }
        if let Some(reference) = reference {
            let original: Vec<_> = (0..self.row_count())
                .map(|row| self.read(row, 0, self.width))
                .collect();
            let mut old = 0;
            for col in 0..reference[0].len() {
                let matches = old < original[0].len()
                    && reference.iter().enumerate().all(|(row, seq)| {
                        let before = original[row][old];
                        let after = seq.as_bytes()[col];
                        before == after || (is_gap(before) && is_gap(after))
                    });
                if matches {
                    old += 1;
                } else if reference.iter().any(|seq| !is_gap(seq.as_bytes()[col])) {
                    return err("Addition changed reference column homology");
                }
            }
            if old != original[0].len() {
                return err("Addition removed reference columns");
            }
        }
        let mut undo = Vec::new();
        if let Some(reference) = reference {
            let width = self.width;
            for (i, seq) in reference.iter().enumerate() {
                self.splice(i, 0, width, seq.as_bytes(), &mut undo);
            }
        }
        for (name, seq) in rows {
            let index = self.rows.len();
            self.rows
                .push(AlnRow::new(name, Arc::new(seq.as_bytes().to_vec())));
            undo.push(Undo::RemoveRow { index });
        }
        self.square(&mut undo);
        self.edited();
        Ok(undo)
    }

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
            self.insert_cells(r, col, &gaps(count), !every_row, &mut undo);
        }
        self.square(&mut undo);
        self.edited();
        Ok(undo)
    }

    /// Insert `residues` before column `col` in `rows`: typing in insert
    /// mode. Like gaps, they use up each row's trailing gaps first unless
    /// every row gets them.
    pub fn insert(
        &mut self,
        rows: &RowRanges,
        col: u64,
        residues: &[u8],
    ) -> Result<Vec<Undo>, EditError> {
        let rows = self.expand(rows)?;
        if col > self.width {
            return err(format!("column {col} outside 0..={}", self.width));
        }
        let every_row = rows.len() == self.rows.len();
        let mut undo = Vec::new();
        for r in rows {
            self.insert_cells(r, col, residues, !every_row, &mut undo);
        }
        self.square(&mut undo);
        self.edited();
        Ok(undo)
    }

    /// Type `residue` over every cell of the block `rows` × `start..end`.
    pub fn fill(
        &mut self,
        rows: &RowRanges,
        start: u64,
        end: u64,
        residue: u8,
    ) -> Result<Vec<Undo>, EditError> {
        let rows = self.expand(rows)?;
        if start >= end || end > self.width {
            return err(format!("columns {start}..{end} outside 0..{}", self.width));
        }
        let cells = vec![residue; usize::try_from(end - start).expect("columns fit in memory")];
        let mut undo = Vec::new();
        for r in rows {
            self.splice(r, start, end, &cells, &mut undo);
        }
        self.square(&mut undo);
        self.edited();
        Ok(undo)
    }

    /// Insert `cells` before `col` in one row; with `use_trailing`, first
    /// remove as many of the row's trailing gaps (after `col`) as cells go in.
    fn insert_cells(
        &mut self,
        row: usize,
        col: u64,
        cells: &[u8],
        use_trailing: bool,
        undo: &mut Vec<Undo>,
    ) {
        if use_trailing {
            let len = self.rows[row].len();
            let used = self.trailing_gaps(row, (cells.len() as u64).min(len - col));
            self.splice(row, len - used, len, &[], undo);
        }
        self.splice(row, col, col, cells, undo);
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

    /// Independent edit snapshot sharing the immutable residue sources.
    #[must_use]
    pub fn snapshot(&self) -> Self {
        Self {
            name: self.name.clone(),
            rows: self.rows.clone(),
            width: self.width,
            preserve_unpadded_lengths: self.preserve_unpadded_lengths,
            version: self.version,
            stated: self.stated.clone(),
            guessed: Mutex::new(None),
            consensus: Mutex::new(ConsensusCache::default()),
            stats: Mutex::new(StatsCache::default()),
        }
    }

    /// Half-open runs containing only gaps across every row.
    /// Ambiguity and missing-data symbols are not gaps.
    // Column indexes fit in usize: `all_gap` holds one flag per column.
    #[allow(clippy::cast_possible_truncation)]
    pub fn gap_column_runs(&self) -> Vec<(u64, u64)> {
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
        runs
    }

    /// Delete every gap-only column in one undoable operation.
    pub fn remove_gap_columns(&mut self) -> (Vec<Undo>, u64) {
        let runs = self.gap_column_runs();
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
        if self
            .rows
            .iter()
            .enumerate()
            .any(|(i, r)| i != row && r.name == name)
        {
            return err(format!("there's already a row named {name:?}"));
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
                    content_len,
                } => {
                    let row = &mut self.rows[*row];
                    row.splice(*pos, pos + len, residues);
                    row.content_len = *content_len;
                }
                Undo::InsertRow { index, row } => self.rows.insert(*index, (**row).clone()),
                Undo::RemoveRow { index } => {
                    self.rows.remove(*index);
                }
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

    /// Composition over all rows, independent of which rows are displayed.
    pub fn column_stats(&self, col: u64) -> Option<ColumnStats> {
        if col >= self.width || self.is_protein() || self.is_mixed() {
            return None;
        }
        let chunk = self.stats_chunk(col / CONSENSUS_CHUNK);
        Some(chunk[(col % CONSENSUS_CHUNK) as usize])
    }

    /// Bounded, versioned whole-alignment composition for a visible interval.
    pub fn column_stats_range(&self, start: u64, end: u64) -> Vec<ColumnStats> {
        let end = end.min(self.width);
        if self.is_protein() || self.is_mixed() || start >= end {
            return Vec::new();
        }
        let Ok(capacity) = usize::try_from(end - start) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(capacity);
        for index in start / CONSENSUS_CHUNK..=(end - 1) / CONSENSUS_CHUNK {
            let chunk = self.stats_chunk(index);
            let base = index * CONSENSUS_CHUNK;
            let bounds = usize::try_from(start.max(base) - base)
                .ok()
                .zip(usize::try_from(end.min(base + chunk.len() as u64) - base).ok());
            let Some((first, last)) = bounds else {
                return Vec::new();
            };
            out.extend_from_slice(&chunk[first..last]);
        }
        out
    }

    fn stats_chunk(&self, index: u64) -> Arc<Vec<ColumnStats>> {
        {
            let mut cache = self
                .stats
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.version != self.version {
                cache.version = self.version;
                cache.chunks.clear();
            }
            if let Some(chunk) = cache.chunks.get(&index) {
                return Arc::clone(chunk);
            }
        }
        let start = index * CONSENSUS_CHUNK;
        let end = (start + CONSENSUS_CHUNK).min(self.width);
        let mut columns: Vec<_> = (start..end).map(|_| ColumnStats::default()).collect();
        let mut buf = Vec::with_capacity(columns.len());
        for row in 0..self.rows.len() {
            buf.clear();
            self.read_into(row, start, end, &mut buf);
            for (column, &base) in columns.iter_mut().zip(&buf) {
                match base.to_ascii_uppercase() {
                    b'A' => column.acgt[0] += 1,
                    b'C' => column.acgt[1] += 1,
                    b'G' => column.acgt[2] += 1,
                    b'T' | b'U' => column.acgt[3] += 1,
                    b'-' | b'.' => column.gap += 1,
                    b'N' | b'?' => column.missing += 1,
                    b'R' | b'Y' | b'S' | b'W' | b'K' | b'M' | b'B' | b'D' | b'H' | b'V' => {
                        column.ambiguous += 1;
                    }
                    _ => column.other += 1,
                }
            }
        }
        let columns = Arc::new(columns);
        let mut cache = self
            .stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.version == self.version {
            cache.chunks.insert(index, Arc::clone(&columns));
        }
        columns
    }

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
        let (protein, mixed) = (self.is_protein(), self.is_mixed());
        let cols: Vec<Consensus> = counts
            .iter()
            .map(|c| {
                if mixed {
                    return Consensus {
                        residue: GAP,
                        agreement: 0.0,
                    };
                }
                if !protein {
                    return dna_consensus(c, u32::try_from(self.rows.len()).unwrap_or(u32::MAX));
                }
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
    fn addition_collisions_list_every_id_once_in_input_order() {
        let mut doc = aln(&["AC", "GT"]);
        let first = doc.row(0).name.clone();
        let second = doc.row(1).name.clone();
        let rows = vec![
            (second.clone(), "AC".into()),
            (first.clone(), "GT".into()),
            (second.clone(), "AT".into()),
        ];
        assert_eq!(
            doc.duplicate_sequence_names(&rows),
            vec![second.clone(), first.clone()]
        );
        let error = doc.add_sequences(&rows, None).err().unwrap().to_string();
        assert!(error.contains(&first) && error.contains(&second));
        assert_eq!(doc.row_count(), 2);
    }

    #[test]
    fn additions_must_match_the_alignment_alphabet() {
        let mut dna = aln(&["ACGTN", "ACGTT"]);
        assert!(
            dna.add_sequences(&[("bad".into(), "ACGTE".into())], None)
                .is_err()
        );
        assert!(
            dna.add_sequences(&[("stop".into(), "ACGT*".into())], None)
                .is_err()
        );
        assert!(
            dna.add_sequences(&[("iupac".into(), "RYKMSWBDHVN".into())], None)
                .is_ok()
        );
        assert_eq!(
            aln(&["NNRYKMSW", "NNBDHVRY"]).alphabet().value,
            Alphabet::Dna
        );
        let protein = aln(&["MKLVE", "MKLIE"]);
        assert!(
            protein
                .validate_additions(&[("valid".into(), "MKLBZX*".into())])
                .is_ok()
        );
        assert!(
            protein
                .validate_additions(&[("bad".into(), "MKL?".into())])
                .is_err()
        );
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
    fn gap_snapshot_is_independent_and_ambiguities_are_not_gaps() {
        let mut d = AlignmentDoc::new(
            "test",
            vec![
                AlnRow::new("a", Arc::new(b"A-.N?-".to_vec())),
                AlnRow::new("b", Arc::new(b"T.-?N-".to_vec())),
            ],
        );
        let snapshot = d.snapshot();
        assert_eq!(snapshot.gap_column_runs(), vec![(1, 3), (5, 6)]);
        let (undo, count) = d.remove_gap_columns();
        assert_eq!(count, 3);
        assert_eq!(d.read(0, 0, d.width()), b"AN?");
        assert_eq!(snapshot.read(0, 0, snapshot.width()), b"A-.N?-");
        d.undo(&undo);
        assert_eq!(d.gap_column_runs(), snapshot.gap_column_runs());
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
    fn dna_consensus_uses_least_inclusive_majority_iupac() {
        let d = aln(&["AAAAN", "AACAR", "GCGAC", "GC-CT", "TG-T-"]);
        // A/G tie below 50% => R (80%); A/C/G below 50% => M (80%).
        assert_eq!(d.consensus_at(0).unwrap().residue, b'R');
        assert_eq!(d.consensus_at(1).unwrap().residue, b'M');
        assert_eq!(d.consensus_at(2).unwrap().residue, b'M');
        assert_eq!(d.consensus_at(3).unwrap().residue, b'A');
        assert_eq!(
            aln(&["R", "R", "A", "C", "T"])
                .consensus_at(0)
                .unwrap()
                .residue,
            b'R'
        );
        assert_eq!(aln(&["A", "G"]).consensus_at(0).unwrap().residue, b'A');
        assert_eq!(aln(&["?", "?", "A"]).consensus_at(0).unwrap().residue, b'N');
        // Stated, so the protein path doesn't depend on the guess.
        let mut protein = aln(&["E", "E", "F", "L", "V"]);
        protein.set_alphabet(Some(Fact {
            value: Alphabet::Protein,
            source: FactSource::User,
        }));
        assert_eq!(protein.consensus_at(0).unwrap().residue, b'E');
    }

    #[test]
    fn consensus_ignores_terminal_and_internal_gaps() {
        let d = aln(&["AC-", "A--", "---", "---", "---"]);
        let c = d.consensus(0, 3);
        assert_eq!(
            c[0],
            Consensus {
                residue: b'A',
                agreement: 1.0
            }
        );
        assert_eq!(
            c[1],
            Consensus {
                residue: b'C',
                agreement: 1.0
            }
        );
        assert_eq!(
            c[2],
            Consensus {
                residue: b'-',
                agreement: 0.0
            }
        );
        assert_eq!(
            aln(&["A", "C", "G", "-", "-", "-"])
                .consensus_at(0)
                .unwrap()
                .residue,
            b'M'
        );
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
    fn composition_keeps_coverage_and_ambiguity_distinct() {
        let d = aln(&[
            "ACGTACGTAA",
            "ACGTACGTAA",
            "ACGTACGTAC",
            "ACGTACGTA-",
            "ACGTACGTAN",
            "ACGTACGTAR",
            "ACGTACGTA?",
        ]);
        let s = d.column_stats(9).unwrap();
        assert_eq!(s.acgt, [2, 1, 0, 0]);
        assert_eq!((s.gap, s.missing, s.ambiguous), (1, 2, 1));
        assert_eq!(s.informative(), 3);
        assert!((s.entropy_bits().unwrap() - 0.918_295_834).abs() < 1e-8);
        assert!(
            aln(&["N", "-"])
                .column_stats(0)
                .unwrap()
                .entropy_bits()
                .is_none()
        );
        assert_eq!(d.column_stats_range(9, 10), vec![s]);
        assert_eq!(d.column_stats_range(10, 11), []);
    }

    #[test]
    fn composition_cache_tracks_alignment_edits() {
        let mut d = aln(&["AA", "CA"]);
        assert_eq!(d.column_stats_range(0, 2)[0].acgt, [1, 1, 0, 0]);
        let undo = d.replace(1, 0, 1, b"A").unwrap();
        assert_eq!(d.column_stats_range(0, 2)[0].acgt, [2, 0, 0, 0]);
        d.undo(&undo);
        assert_eq!(d.column_stats_range(0, 2)[0].acgt, [1, 1, 0, 0]);
    }

    #[test]
    fn protein_detection() {
        assert_eq!(
            aln(&["ACGTNACGT-", "ACG-TACGT"]).alphabet().value,
            Alphabet::Dna
        );
        assert_eq!(
            aln(&["MKLVWQERST", "MKLI-QERST"]).alphabet().value,
            Alphabet::Protein
        );
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
                0 if rnd(2) == 0 => d.replace(a as usize, s, e, b"ACG"),
                0 if rnd(2) == 0 => d.insert(&[(a, b)], s, b"TT"),
                0 => d.fill(&[(a, b)], s, e, b'-'),
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

    /// Typing in insert mode, a residue at a time, ends the same as inserting
    /// the whole burst at once.
    #[test]
    fn consecutive_inserts_merge() {
        let mut x = 0x1234_5678u64;
        let mut rnd = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n.max(1)
        };
        for _ in 0..300 {
            let rows: Vec<String> = (0..3)
                .map(|_| {
                    let mut row: String = (0..2 + rnd(8))
                        .map(|_| "AC-".as_bytes()[rnd(3) as usize] as char)
                        .collect();
                    row.extend((0..rnd(4)).map(|_| '-'));
                    row
                })
                .collect();
            let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
            let row = rnd(3) as u32;
            let mut one = aln(&refs);
            let col = rnd(one.width() + 1);
            let typed = &b"GTGA"[..=(rnd(4) as usize)];
            let mut steps = aln(&refs);
            for (i, b) in typed.iter().enumerate() {
                steps
                    .insert(&[(row, row + 1)], col + i as u64, &[*b])
                    .unwrap();
            }
            one.insert(&[(row, row + 1)], col, typed).unwrap();
            assert_eq!(text(&one), text(&steps), "{refs:?} {row} {col}");
        }
        let mut d = aln(&["AC--", "ACGT"]);
        d.insert(&[(0, 1)], 1, b"G").unwrap();
        assert_eq!(text(&d), ["AGC-", "ACGT"]);
        // Into every row it's a new column.
        d.insert(&[(0, 2)], 0, b"T").unwrap();
        assert_eq!(text(&d), ["TAGC-", "TACGT"]);
        d.fill(&[(0, 2)], 1, 3, b'-').unwrap();
        assert_eq!(text(&d), ["T--C-", "T--GT"]);
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
