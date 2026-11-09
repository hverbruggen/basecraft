//! Transient document Find, independent of edit selection and persistence.
//!
//! Names are Unicode-lowercased literal substrings. Motifs are ASCII,
//! case-insensitive, overlapping and ignore `-`/`.` in the document (not in
//! the query). DNA/RNA symbols match when their IUPAC sets intersect; T = U.
//! Protein letters and `*` match literally, including B/Z/X. Unknown document
//! symbols break a motif. Reverse complement adds the other strand, with
//! palindromes reported once. Coordinates are zero-based, half-open display
//! spans, including intervening gaps. No circular-origin wrapping is implied.

use basecraft_docs::alignment::{dna_mask, is_gap};
use basecraft_docs::{AlignmentDoc, SequenceDoc};
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

/// Bound memory for extremely broad searches; the UI explicitly reports this.
pub const MATCH_LIMIT: usize = 100_000;
const CHUNK: u16 = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Name,
    Sequence,
    Annotation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub row: usize,
    pub start: u64,
    pub end: u64,
    pub kind: Kind,
    pub reverse: bool,
    pub label: Arc<str>,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: usize,
    pub name: String,
    pub bases: Vec<u8>,
}

/// A display-row snapshot materialized on the background scan thread.
pub type RowBuilder = Box<dyn FnOnce(&AtomicBool) -> Vec<Row> + Send>;

pub enum Input {
    Alignment(AlignmentDoc),
    Sequence(SequenceDoc),
    Names(Vec<(usize, String)>),
    /// Sanger rows already in the shared display-column coordinate system.
    Rows(Vec<Row>),
    /// Expensive display-row derivation (e.g. reference consensus) runs on the
    /// scan thread, with the same cancellation token as motif matching.
    DeferredRows(RowBuilder),
}

#[derive(Default, Debug)]
pub struct Results {
    pub hits: Vec<Hit>,
    pub explanation: String,
    pub nucleotide: bool,
    pub limited: bool,
}

struct Pattern {
    masks: Vec<Vec<u64>>,
    state: Vec<u64>,
    positions: VecDeque<u64>,
    len: usize,
}
impl Pattern {
    fn new(query: &[u8], protein: bool) -> Self {
        let words = query.len().div_ceil(64);
        let mut masks = vec![vec![0; words]; 256];
        for symbol in 0u8..=255 {
            for (i, &q) in query.iter().enumerate() {
                let matches = if protein {
                    symbol.eq_ignore_ascii_case(&q)
                } else {
                    dna_mask(symbol) & dna_mask(q) != 0
                };
                if matches {
                    masks[usize::from(symbol)][i / 64] |= 1 << (i % 64);
                }
            }
        }
        Self {
            masks,
            state: vec![0; words],
            positions: VecDeque::new(),
            len: query.len(),
        }
    }
    fn feed(&mut self, symbol: u8, position: u64) -> Option<(u64, u64)> {
        if is_gap(symbol) {
            return None;
        }
        let mut carry = 1;
        for (word, mask) in self.state.iter_mut().zip(&self.masks[usize::from(symbol)]) {
            let next = *word >> 63;
            *word = ((*word << 1) | carry) & mask;
            carry = next;
        }
        self.positions.push_back(position);
        if self.positions.len() > self.len {
            self.positions.pop_front();
        }
        (self.state[(self.len - 1) / 64] & (1 << ((self.len - 1) % 64)) != 0)
            .then(|| (*self.positions.front().unwrap_or(&position), position + 1))
    }
}

struct Scanner<'a> {
    query_lower: String,
    forward: Option<Pattern>,
    reverse: Option<Pattern>,
    result: Results,
    cancel: &'a AtomicBool,
}
impl<'a> Scanner<'a> {
    fn new(
        query: &str,
        reverse: bool,
        protein: bool,
        names_only: bool,
        cancel: &'a AtomicBool,
    ) -> Self {
        let mut result = Results {
            nucleotide: !protein && !names_only,
            ..Results::default()
        };
        let valid = !query.is_empty()
            && query.bytes().all(|b| {
                if protein {
                    b.is_ascii_alphabetic() || b == b'*'
                } else {
                    dna_mask(b) != 0
                }
            });
        let mut forward = None;
        let mut reverse_pattern = None;
        if !names_only {
            if !valid {
                result.explanation = if protein {
                    "Sequence query requires protein letters or * (no spaces or gaps)"
                } else {
                    "Sequence query requires IUPAC DNA/RNA letters (no spaces or gaps)"
                }
                .into();
            } else if reverse && protein {
                result.explanation =
                    "Reverse complement is unavailable for protein sequences".into();
            } else {
                forward = Some(Pattern::new(query.as_bytes(), protein));
                if reverse {
                    let canonical: Vec<u8> = query
                        .bytes()
                        .map(|b| {
                            if b.eq_ignore_ascii_case(&b'U') {
                                b'T'
                            } else {
                                b.to_ascii_uppercase()
                            }
                        })
                        .collect();
                    let rc = crate::sanger_map::reverse_complement(&canonical);
                    // Compare masks, so RNA T/U equivalence also deduplicates.
                    if !rc
                        .iter()
                        .zip(query.bytes())
                        .all(|(&a, b)| dna_mask(a) == dna_mask(b))
                    {
                        reverse_pattern = Some(Pattern::new(&rc, false));
                    }
                }
            }
        }
        Self {
            query_lower: query.to_lowercase(),
            forward,
            reverse: reverse_pattern,
            result,
            cancel,
        }
    }
    fn stopped(&self) -> bool {
        self.result.limited || self.cancel.load(Ordering::Relaxed)
    }
    fn push(&mut self, hit: Hit) {
        if self.result.hits.len() == MATCH_LIMIT {
            self.result.limited = true;
        } else {
            self.result.hits.push(hit);
        }
    }
    fn name(&mut self, row: usize, name: &str, kind: Kind, span: (u64, u64)) {
        if name.to_lowercase().contains(&self.query_lower) {
            self.push(Hit {
                row,
                start: span.0,
                end: span.1,
                kind,
                reverse: false,
                label: name.into(),
            });
        }
    }
    fn begin_row(&mut self) {
        for pattern in [&mut self.forward, &mut self.reverse].into_iter().flatten() {
            pattern.state.fill(0);
            pattern.positions.clear();
        }
    }
    fn bases(&mut self, row: usize, label: &Arc<str>, offset: u64, bases: &[u8]) {
        if self.forward.is_none() {
            return;
        }
        for (i, &b) in bases.iter().enumerate() {
            if i % 1024 == 0 && self.stopped() {
                break;
            }
            let pos = offset + i as u64;
            let forward = self.forward.as_mut().and_then(|p| p.feed(b, pos));
            let reverse = self.reverse.as_mut().and_then(|p| p.feed(b, pos));
            for (span, reverse) in [(forward, false), (reverse, true)] {
                if let Some((start, end)) = span {
                    self.push(Hit {
                        row,
                        start,
                        end,
                        kind: Kind::Sequence,
                        reverse,
                        label: label.clone(),
                    });
                }
            }
        }
    }
}

/// Search an immutable snapshot; cancellation is checked between chunks/rows.
#[must_use]
pub fn search(input: Input, query: &str, reverse: bool, cancel: &AtomicBool) -> Results {
    if query.is_empty() {
        return Results::default();
    }
    let input = match input {
        Input::DeferredRows(rows) => Input::Rows(rows(cancel)),
        input => input,
    };
    let protein = match &input {
        Input::Alignment(d) => d.is_protein(),
        // Same conservative alphabet convention as AlignmentDoc: a letter
        // outside the nucleotide alphabet identifies protein. Inspect the full
        // sequence here, off the UI thread; SequenceDoc has no alphabet field.
        Input::Sequence(d) => {
            let mut protein = false;
            for start in (0..d.len()).step_by(usize::from(CHUNK)) {
                if cancel.load(Ordering::Relaxed) {
                    return Results::default();
                }
                if d.read(start, (start + u64::from(CHUNK)).min(d.len()))
                    .iter()
                    .any(|&b| b.is_ascii_alphabetic() && dna_mask(b) == 0)
                {
                    protein = true;
                    break;
                }
            }
            protein
        }
        _ => false,
    };
    let mut s = Scanner::new(
        query,
        reverse,
        protein,
        matches!(input, Input::Names(_)),
        cancel,
    );
    match input {
        Input::Names(rows) => {
            for (id, name) in rows {
                if s.stopped() {
                    break;
                }
                s.name(id, &name, Kind::Name, (0, 0));
            }
        }
        Input::Alignment(d) => {
            for row in 0..d.row_count() {
                if s.stopped() {
                    break;
                }
                let label: Arc<str> = d.row(row).name.as_str().into();
                s.name(row, &label, Kind::Name, (0, 0));
                s.begin_row();
                if s.forward.is_some() {
                    for start in (0..d.width()).step_by(usize::from(CHUNK)) {
                        if s.stopped() {
                            break;
                        }
                        s.bases(
                            row,
                            &label,
                            start,
                            &d.read(row, start, (start + u64::from(CHUNK)).min(d.width())),
                        );
                    }
                }
            }
        }
        Input::Sequence(d) => {
            let label: Arc<str> = d.name.as_str().into();
            if s.forward.is_some() {
                for start in (0..d.len()).step_by(usize::from(CHUNK)) {
                    if s.stopped() {
                        break;
                    }
                    s.bases(
                        0,
                        &label,
                        start,
                        &d.read(start, (start + u64::from(CHUNK)).min(d.len())),
                    );
                }
            }
            for (id, f) in d.features() {
                if s.stopped() {
                    break;
                }
                let start = f
                    .location
                    .segments
                    .iter()
                    .map(|x| x.start)
                    .min()
                    .unwrap_or(0);
                let end = f
                    .location
                    .segments
                    .iter()
                    .map(|x| x.end)
                    .max()
                    .unwrap_or(start);
                let names = f
                    .qualifiers
                    .iter()
                    .filter(|q| {
                        ["Name", "gene", "locus_tag", "product", "label", "ID"]
                            .contains(&q.key.as_str())
                    })
                    .filter_map(|q| q.value.as_deref())
                    .collect::<Vec<_>>();
                if names
                    .iter()
                    .any(|name| name.to_lowercase().contains(&s.query_lower))
                {
                    s.push(Hit {
                        row: id.0 as usize,
                        start,
                        end,
                        kind: Kind::Annotation,
                        reverse: false,
                        label: names.join(" · ").into(),
                    });
                }
            }
            s.result.hits.sort_by_key(|h| (h.start, h.end, h.row));
        }
        Input::DeferredRows(_) => return Results::default(),
        Input::Rows(rows) => {
            for row in rows {
                if s.stopped() {
                    break;
                }
                s.begin_row();
                let label: Arc<str> = row.name.into();
                s.bases(row.id, &label, 0, &row.bases);
            }
        }
    }
    s.result
}

/// A cancellable transient scan per view. Dropping the old receiver prevents
/// stale query/document results from ever being accepted, even at equal versions.
#[derive(Default)]
pub struct Session {
    revision: u64,
    query: String,
    reverse: bool,
    key: Option<String>,
    pending: Option<(Arc<AtomicBool>, mpsc::Receiver<Results>)>,
    pub results: Results,
    pub current: usize,
}
impl Session {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn needs_update(&self, key: &str) -> bool {
        self.active() && self.key.as_deref() != Some(key)
    }
    pub fn active(&self) -> bool {
        !self.query.is_empty()
    }
    pub fn busy(&self) -> bool {
        self.active() && (self.pending.is_some() || self.key.is_none())
    }
    pub fn hit(&self) -> Option<&Hit> {
        self.results.hits.get(self.current)
    }
    fn cancel(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        if let Some((stop, _)) = self.pending.take() {
            stop.store(true, Ordering::Relaxed);
        }
        self.key = None;
        self.results = Results::default();
        self.current = 0;
    }
    pub fn set_query(&mut self, query: String, reverse: bool) {
        if self.query != query || self.reverse != reverse {
            self.cancel();
            self.query = query;
            self.reverse = reverse;
        }
    }
    pub fn invalidate(&mut self, key: &str) {
        if self.key.as_deref().is_some_and(|old| old != key) {
            self.cancel();
        }
    }
    pub fn update(&mut self, key: String, snapshot: impl FnOnce() -> Input) {
        self.invalidate(&key);
        if !self.active() || self.key.is_some() {
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        let input = snapshot();
        let query = self.query.clone();
        let reverse = self.reverse;
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        self.key = Some(key);
        self.pending = Some((stop, receiver));
        // A failed OS thread allocation is an ordinary visible error.
        if let Err(error) = std::thread::Builder::new()
            .name("document-find".into())
            .spawn(move || {
                let result = search(input, &query, reverse, &worker_stop);
                if !worker_stop.load(Ordering::Relaxed) {
                    let _ = sender.send(result);
                }
            })
        {
            self.pending = None;
            self.results.explanation = format!("Cannot start Find: {error}");
        }
    }
    /// True only when a fresh scan completes; caller reveals its first hit.
    pub fn poll(&mut self) -> bool {
        let Some((_, receiver)) = &self.pending else {
            return false;
        };
        match receiver.try_recv() {
            Ok(results) => {
                self.revision = self.revision.wrapping_add(1);
                self.pending = None;
                self.results = results;
                self.current = 0;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.revision = self.revision.wrapping_add(1);
                self.pending = None;
                self.results.explanation = "Find could not finish".into();
                false
            }
        }
    }
    pub fn step(&mut self, backward: bool) {
        let n = self.results.hits.len();
        if n > 0 {
            self.revision = self.revision.wrapping_add(1);
            self.current = if backward {
                (self.current + n - 1) % n
            } else {
                (self.current + 1) % n
            };
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Outline rather than another selection fill; current match gets a double line.
pub fn outline(
    frame: &mut crate::Frame,
    rect: (f32, f32, f32, f32),
    theme: &crate::Theme,
    current: bool,
) {
    let (x, y, w, h) = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let color = theme.foreground;
    frame.rect(x, y, w, 1.0, color);
    frame.rect(x, y + h - 1.0, w, 1.0, color);
    frame.rect(x, y, 1.0, h, color);
    frame.rect(x + w - 1.0, y, 1.0, h, color);
    if current {
        frame.rect(x, y + h - 4.0, w, 2.0, theme.accent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_docs::alignment::AlnRow;
    use basecraft_protocol::{Feature, Location, Qualifier, Strand};

    fn rows(bases: &[u8]) -> Input {
        Input::Rows(vec![Row {
            id: 4,
            name: "Read 2".into(),
            bases: bases.to_vec(),
        }])
    }
    fn run(bases: &[u8], query: &str, rc: bool) -> Results {
        search(rows(bases), query, rc, &AtomicBool::new(false))
    }
    #[test]
    fn find_gaps_overlaps_and_display_coordinates() {
        let r = run(b"A-A.AA", "AAA", false);
        assert_eq!(
            r.hits
                .iter()
                .map(|h| (h.row, h.start, h.end))
                .collect::<Vec<_>>(),
            vec![(4, 0, 5), (4, 2, 6)]
        );
        assert!(r.hits.iter().all(|h| h.label.as_ref() == "Read 2"));
        assert_eq!(run(b"AA?AA", "AAA", false).hits, []);
        assert_eq!(run(b"AA AA", "AAA", false).hits, []);
    }
    #[test]
    fn find_iupac_intersection_rna_reverse_and_palindromes() {
        assert_eq!(
            run(b"AGCTN", "R", false)
                .hits
                .iter()
                .map(|h| h.start)
                .collect::<Vec<_>>(),
            vec![0, 1, 4]
        );
        assert_eq!(run(b"UuT", "t", false).hits.len(), 3);
        let r = run(b"CAT", "aug", true);
        assert_eq!(r.hits.len(), 1);
        assert!(r.hits[0].reverse);
        assert_eq!(run(b"ACGT", "acgt", true).hits.len(), 1);
        assert_eq!(run(b"R", "y", true).hits.len(), 1);
        assert!(
            run(b"AAAA", "A-A", false)
                .explanation
                .contains("no spaces or gaps")
        );
        assert_eq!(run(b"AAAA", "a a", false).hits, []);
    }
    #[test]
    fn find_long_motifs_cross_word_and_read_chunk_boundaries() {
        let query = "A".repeat(130);
        let r = run(&[b'A'; 132], &query, false);
        assert_eq!(r.hits.len(), 3);
        assert_eq!((r.hits[2].start, r.hits[2].end), (2, 132));
        let mut bases = vec![b'C'; usize::from(CHUNK) - 2];
        bases.extend_from_slice(b"A--A.AA");
        let doc = SequenceDoc::new("sample", Arc::new(bases), vec![], false);
        let r = search(Input::Sequence(doc), "AAAA", false, &AtomicBool::new(false));
        assert_eq!(r.hits.len(), 1);
        assert_eq!(
            (r.hits[0].start, r.hits[0].end),
            (u64::from(CHUNK) - 2, u64::from(CHUNK) + 5)
        );
    }
    #[test]
    fn find_names_annotations_and_protein_symbols() {
        let doc = AlignmentDoc::new(
            "proteins",
            vec![AlnRow::new("Species R", Arc::new(b"MRNX*R".to_vec()))],
        );
        assert!(doc.is_protein());
        let r = search(
            Input::Alignment(doc.snapshot()),
            "R",
            false,
            &AtomicBool::new(false),
        );
        assert_eq!(
            r.hits.iter().map(|h| (h.kind, h.start)).collect::<Vec<_>>(),
            vec![(Kind::Name, 0), (Kind::Sequence, 1), (Kind::Sequence, 5)]
        );
        let r = search(Input::Alignment(doc), "R", true, &AtomicBool::new(false));
        assert!(r.explanation.contains("unavailable"));
        assert!(r.hits.iter().all(|h| h.kind == Kind::Name));
        let r = search(
            Input::Names(vec![(42, "Älga beta".into()), (9, "Other".into())]),
            "ÄLGA",
            false,
            &AtomicBool::new(false),
        );
        assert_eq!(r.hits[0].row, 42);
        assert!(!r.nucleotide);
        let feature = Feature {
            kind: "CDS".into(),
            location: Location::simple(3, 9, Strand::Reverse),
            qualifiers: vec![
                Qualifier::new("Name", "display label"),
                Qualifier::new("gene", "rbcL"),
            ],
        };
        let d = SequenceDoc::new(
            "sample",
            Arc::new(b"ACGTACGTACGT".to_vec()),
            vec![feature],
            false,
        );
        let r = search(Input::Sequence(d), "RBCL", false, &AtomicBool::new(false));
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].kind, Kind::Annotation);
        assert_eq!((r.hits[0].start, r.hits[0].end), (3, 9));
        assert_ne!(r.explanation, "");
    }
    #[test]
    fn find_late_query_document_and_edit_results_are_discarded() {
        let mut session = Session::default();
        session.set_query("AA".into(), false);
        session.key = Some("document-a:version-0".into());
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        session.pending = Some((stop.clone(), receiver));
        session.set_query("CC".into(), false);
        assert!(stop.load(Ordering::Relaxed));
        assert!(sender.send(run(b"AAAA", "AA", false)).is_err());
        assert!(!session.poll());
        assert!(session.hit().is_none());
        for new_key in ["document-b:version-0", "document-a:version-1"] {
            session.key = Some("document-a:version-0".into());
            session.results = run(b"AAAA", "AA", false);
            session.invalidate(new_key);
            assert!(session.hit().is_none());
            assert!(session.busy());
        }
        session.results = run(b"AAAA", "AA", false);
        session.step(true);
        assert_eq!(session.current, 2);
        session.step(false);
        assert_eq!(session.current, 0);
        session.set_query(String::new(), false);
        assert!(session.hit().is_none());
        assert!(!session.busy());
    }
    #[test]
    fn find_scan_completion_and_cancellation() {
        let mut session = Session::default();
        session.set_query("AA".into(), false);
        let caller = std::thread::current().id();
        session.update("document:0".into(), || {
            Input::DeferredRows(Box::new(move |_| {
                assert_ne!(std::thread::current().id(), caller);
                vec![Row {
                    id: 0,
                    name: "consensus".into(),
                    bases: b"AAAA".to_vec(),
                }]
            }))
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !session.poll() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(session.results.hits.len(), 3);
        assert!(!session.busy());
        assert_eq!(
            search(rows(b"AAAA"), "AA", false, &AtomicBool::new(true)).hits,
            []
        );
        let r = run(&vec![b'A'; MATCH_LIMIT + 1], "A", false);
        assert!(r.limited);
        assert_eq!(r.hits.len(), MATCH_LIMIT);
    }
    #[test]
    fn find_reveal_and_highlight_do_not_change_scientific_selection() {
        let d = AlignmentDoc::new(
            "sample",
            vec![AlnRow::new("taxon", Arc::new(b"ACGTACGT".to_vec()))],
        );
        let mut view = crate::AlignmentView::new(800.0, 500.0);
        view.sync(&d);
        let y = view.y_of(0) + view.row_height() * 0.5;
        view.press(view.x_of(2.1), y, false);
        view.drag_to(view.x_of(4.1), y);
        view.release();
        assert_eq!(
            view.selection,
            Some(crate::alignment::Block {
                rows: (0, 1),
                cols: (2, 5)
            })
        );
        let mut find = Session::default();
        find.results = search(
            Input::Alignment(d.snapshot()),
            "ACG",
            false,
            &AtomicBool::new(false),
        );
        let selection = view.selection;
        let cursor = view.cursor;
        view.reveal((0, 4));
        let mut frame = crate::Frame::new();
        view.build(&d, &crate::Theme::default(), &mut frame);
        let before = frame.len();
        view.draw_find(&find, &crate::Theme::default(), &mut frame);
        assert!(frame.len() > before);
        assert_eq!(view.selection, selection);
        assert_eq!(view.cursor, cursor);
        let mut export = crate::Frame::new();
        view.build(&d, &crate::Theme::default(), &mut export);
        assert_eq!(export.len(), before);
        assert_eq!(d.read(0, 0, 8), b"ACGTACGT");
    }
    #[test]
    fn find_large_alignment_performance_evidence() {
        let bases = Arc::new(vec![b'C'; 10_000]);
        let d = AlignmentDoc::new(
            "large",
            (0..1000)
                .map(|i| AlnRow::new(format!("taxon-{i}"), bases.clone()))
                .collect(),
        );
        let started = std::time::Instant::now();
        let snapshot = d.snapshot();
        let snapshot_time = started.elapsed();
        let started = std::time::Instant::now();
        let r = search(
            Input::Alignment(snapshot),
            "AAAAAAA",
            false,
            &AtomicBool::new(false),
        );
        eprintln!(
            "Find 10M cells: snapshot {snapshot_time:?}; scan {:?}",
            started.elapsed()
        );
        assert_eq!(r.hits, []);
        let started = std::time::Instant::now();
        let r = search(
            Input::Names((0..100_000).map(|i| (i, format!("taxon-{i}"))).collect()),
            "99999",
            false,
            &AtomicBool::new(false),
        );
        eprintln!(
            "Find 100k names including snapshot: {:?}",
            started.elapsed()
        );
        assert_eq!(r.hits.len(), 1);
    }
}
