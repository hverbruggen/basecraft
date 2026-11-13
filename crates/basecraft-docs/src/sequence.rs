//! An open sequence document: residues in a piece table, features with
//! stable ids, and the history of coordinate changes.

use std::sync::{Arc, Mutex};

use basecraft_io::fasta::IndexedFasta;
use basecraft_io::{genbank, gff3};
use basecraft_protocol::Feature;
use basecraft_protocol::facts::{Alphabet, Fact};
use basecraft_query::{IntervalIndex, SeqSummary};
use serde::{Deserialize, Serialize};

use crate::coord::{Change, map_location};
use crate::piece::{PieceTable, Residues};

/// Residues read to guess a sequence's alphabet.
const GUESS_SAMPLE: u64 = 100_000;

/// Stable for the life of a document: removing a feature leaves a hole, so
/// ids in the log, in selections and in plugin results stay valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FeatureId(pub u32);

/// GenBank header data carried through for writing the file back.
#[derive(Clone, Debug, Default)]
pub struct GenbankMeta {
    pub molecule: String,
    pub division: String,
    pub date: String,
    pub header: Vec<String>,
}

/// What a residue edit did, with enough to undo it exactly.
#[derive(Clone, Debug)]
pub struct EditResult {
    pub change: Change,
    pub removed: Vec<u8>,
    /// Features overlapping the edited range, as they were before. Undo puts
    /// them back verbatim, because a trimmed feature can't regrow from an
    /// inverse insertion alone.
    pub before: Vec<(FeatureId, Feature)>,
}

pub struct SequenceDoc {
    pub name: String,
    pub circular: bool,
    pub genbank: Option<GenbankMeta>,
    seq: PieceTable,
    features: Vec<Option<Feature>>,
    version: u64,
    /// The letters of the sequence: from the side file, or guessed.
    alphabet: Fact<Alphabet>,
    /// `(version after the change, change)`, oldest first.
    changes: Vec<(u64, Change)>,
    index: Mutex<Option<(u64, Arc<IntervalIndex>)>>,
    summary: Mutex<Option<(u64, Arc<SeqSummary>)>>,
}

impl Clone for SequenceDoc {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            circular: self.circular,
            genbank: self.genbank.clone(),
            seq: self.seq.clone(),
            features: self.features.clone(),
            version: self.version,
            alphabet: self.alphabet.clone(),
            changes: self.changes.clone(),
            index: Mutex::new(None),
            summary: Mutex::new(None),
        }
    }
}

/// One record of an indexed FASTA file as a residue source.
pub struct FastaResidues {
    pub file: Arc<IndexedFasta>,
    pub record: usize,
}

impl Residues for FastaResidues {
    fn len(&self) -> u64 {
        self.file.index.records[self.record].len
    }

    fn read_into(&self, start: u64, end: u64, out: &mut Vec<u8>) {
        // The piece table only asks for ranges inside the record.
        self.file.index.records[self.record]
            .read_into(self.file.bytes(), start, end, out)
            .expect("piece table read inside record bounds");
    }
}

impl SequenceDoc {
    pub fn new(
        name: impl Into<String>,
        residues: Arc<dyn Residues>,
        features: Vec<Feature>,
        circular: bool,
    ) -> Self {
        let seq = PieceTable::new(residues);
        let alphabet = Alphabet::guessed(&seq.read(0, GUESS_SAMPLE));
        Self {
            name: name.into(),
            circular,
            genbank: None,
            seq,
            features: features.into_iter().map(Some).collect(),
            version: 0,
            alphabet,
            changes: Vec::new(),
            index: Mutex::new(None),
            summary: Mutex::new(None),
        }
    }

    /// The letters of the sequence, and who said so.
    pub fn alphabet(&self) -> &Fact<Alphabet> {
        &self.alphabet
    }

    /// Use the alphabet a side file or the user states; `None` guesses
    /// again from the residues.
    pub fn set_alphabet(&mut self, stated: Option<Fact<Alphabet>>) {
        self.alphabet =
            stated.unwrap_or_else(|| Alphabet::guessed(&self.seq.read(0, GUESS_SAMPLE)));
    }

    pub fn from_genbank(rec: genbank::Record) -> Self {
        let mut doc = Self::new(rec.name, Arc::new(rec.sequence), rec.features, rec.circular);
        doc.genbank = Some(GenbankMeta {
            molecule: rec.molecule,
            division: rec.division,
            date: rec.date,
            header: rec.header,
        });
        doc
    }

    pub fn from_fasta(
        file: Arc<IndexedFasta>,
        record: usize,
        gff: Option<gff3::SeqFeatures>,
    ) -> Self {
        let name = file.index.records[record].name.clone();
        let (features, circular) = gff.map_or((Vec::new(), false), |g| (g.features, g.circular));
        Self::new(
            name,
            Arc::new(FastaResidues { file, record }),
            features,
            circular,
        )
    }

    pub fn len(&self) -> u64 {
        self.seq.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seq.is_empty()
    }

    /// Increases with every edit, residues or features.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn read(&self, start: u64, end: u64) -> Vec<u8> {
        self.seq.read(start, end)
    }

    pub fn read_into(&self, start: u64, end: u64, out: &mut Vec<u8>) {
        self.seq.read_into(start, end, out);
    }

    pub fn piece_table(&self) -> &PieceTable {
        &self.seq
    }

    /// Coordinate changes made after `version`, oldest first.
    pub fn changes_since(&self, version: u64) -> impl Iterator<Item = &Change> {
        let i = self.changes.partition_point(|(v, _)| *v <= version);
        self.changes[i..].iter().map(|(_, c)| c)
    }

    // Features.

    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.features.get(id.0 as usize).and_then(Option::as_ref)
    }

    pub fn features(&self) -> impl Iterator<Item = (FeatureId, &Feature)> {
        self.features
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.as_ref().map(|f| (Self::id(i), f)))
    }

    /// Stable feature slots recorded alongside recovery checkpoints.
    pub fn feature_slots(&self) -> Vec<bool> {
        self.features.iter().map(Option::is_some).collect()
    }

    pub fn restore_feature_slots(
        &mut self,
        slots: &[bool],
    ) -> Result<(), crate::alignment::EditError> {
        if slots.iter().filter(|present| **present).count() != self.feature_count() {
            return Err(crate::alignment::EditError(
                "Recovery feature slots differ".into(),
            ));
        }
        let mut features = std::mem::take(&mut self.features).into_iter().flatten();
        self.features = slots
            .iter()
            .map(|present| if *present { features.next() } else { None })
            .collect();
        self.index = Mutex::new(None);
        Ok(())
    }

    pub fn feature_count(&self) -> usize {
        self.features.iter().flatten().count()
    }

    fn id(i: usize) -> FeatureId {
        FeatureId(u32::try_from(i).expect("fewer than 2^32 features"))
    }

    /// Features overlapping `start..end`, by id.
    pub fn features_in(&self, start: u64, end: u64) -> Vec<FeatureId> {
        let index = self.index();
        let mut ids = Vec::new();
        index.query(start, end, &mut ids);
        // Multi-segment features appear once per overlapping segment.
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter().map(FeatureId).collect()
    }

    /// The interval index for the current version, built on demand.
    pub fn index(&self) -> Arc<IntervalIndex> {
        let mut guard = self
            .index
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((v, idx)) = guard.as_ref()
            && *v == self.version
        {
            return Arc::clone(idx);
        }
        let mut items = Vec::with_capacity(self.features.len());
        for (i, f) in self.features.iter().enumerate() {
            let Some(f) = f else { continue };
            // One entry per segment, so a gene spanning the origin of a
            // circular sequence is found at both ends.
            for s in &f.location.segments {
                items.push((s.start, s.end, Self::id(i).0));
            }
        }
        let idx = Arc::new(IntervalIndex::new(items));
        *guard = Some((self.version, Arc::clone(&idx)));
        idx
    }

    /// GC summary for the current version, built on demand.
    pub fn summary(&self) -> Arc<SeqSummary> {
        let mut guard = self
            .summary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((v, s)) = guard.as_ref()
            && *v == self.residue_version()
        {
            return Arc::clone(s);
        }
        let s = Arc::new(SeqSummary::build(self.len(), 256, |a, b, buf| {
            self.seq.read_into(a, b, buf);
        }));
        *guard = Some((self.residue_version(), Arc::clone(&s)));
        s
    }

    /// Version of the last residue change; feature edits don't count.
    fn residue_version(&self) -> u64 {
        self.changes.last().map_or(0, |(v, _)| *v)
    }

    pub fn add_feature(&mut self, f: Feature) -> FeatureId {
        self.features.push(Some(f));
        self.version += 1;
        Self::id(self.features.len() - 1)
    }

    /// Put a feature back under a given id, for undo and replay.
    pub fn restore_feature(&mut self, id: FeatureId, f: Option<Feature>) {
        let i = id.0 as usize;
        if i >= self.features.len() {
            self.features.resize(i + 1, None);
        }
        self.features[i] = f;
        self.version += 1;
    }

    pub fn remove_feature(&mut self, id: FeatureId) -> Option<Feature> {
        let old = self.features.get_mut(id.0 as usize)?.take();
        self.version += 1;
        old
    }

    pub fn update_feature(&mut self, id: FeatureId, f: Feature) -> Option<Feature> {
        let slot = self.features.get_mut(id.0 as usize)?;
        let old = slot.replace(f);
        self.version += 1;
        old
    }

    // Residues.

    pub fn insert(&mut self, pos: u64, residues: &[u8]) -> EditResult {
        self.replace(pos, pos, residues)
    }

    pub fn delete(&mut self, start: u64, end: u64) -> EditResult {
        self.replace(start, end, &[])
    }

    /// Replace `start..end` with `residues` and move features accordingly.
    pub fn replace(&mut self, start: u64, end: u64, residues: &[u8]) -> EditResult {
        let removed = self.seq.delete(start, end);
        self.seq.insert(start, residues);
        let change = Change {
            pos: start,
            deleted: end - start,
            inserted: residues.len() as u64,
        };
        let mut before = Vec::new();
        for (i, slot) in self.features.iter_mut().enumerate() {
            let Some(f) = slot else { continue };
            let Some((fs, fe)) = f.location.bounds() else {
                continue;
            };
            if fe < start {
                continue;
            }
            if fs > end || (fs == end && change.deleted > 0) {
                // Entirely downstream: a pure shift, which an inverse edit
                // reverses exactly.
                let shift = |p: u64| p - change.deleted + change.inserted;
                for s in &mut f.location.segments {
                    s.start = shift(s.start);
                    s.end = shift(s.end);
                }
                continue;
            }
            before.push((Self::id(i), f.clone()));
            match map_location(&f.location, &change) {
                Some(loc) => f.location = loc,
                None => *slot = None,
            }
        }
        self.version += 1;
        self.changes.push((self.version, change));
        EditResult {
            change,
            removed,
            before,
        }
    }

    // Saving.

    pub fn to_genbank(&self) -> genbank::Record {
        let mut record = self.genbank_metadata();
        record.sequence = self.read(0, self.len());
        record.features = self
            .features()
            .map(|(_, feature)| feature.clone())
            .collect();
        record
    }

    /// Header for a streaming writer; features and sequence bytes stay borrowed.
    pub fn genbank_metadata(&self) -> genbank::Record {
        let meta = self.genbank.clone().unwrap_or_else(|| GenbankMeta {
            molecule: "DNA".into(),
            division: "UNK".into(),
            date: "01-JAN-1970".into(),
            header: Vec::new(),
        });
        genbank::Record {
            name: self.name.clone(),
            molecule: meta.molecule,
            circular: self.circular,
            division: meta.division,
            date: meta.date,
            header: meta.header,
            features: Vec::new(),
            sequence: Vec::new(),
        }
    }

    pub fn to_gff(&self) -> gff3::SeqFeatures {
        gff3::SeqFeatures {
            seqid: self.name.clone(),
            circular: self.circular,
            features: self.features().map(|(_, f)| f.clone()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_protocol::{Location, Qualifier, Strand};

    fn doc() -> SequenceDoc {
        let seq: Vec<u8> = b"ACGT".iter().copied().cycle().take(1000).collect();
        let gene = |s, e, name: &str| Feature {
            kind: "gene".into(),
            location: Location::simple(s, e, Strand::Forward),
            qualifiers: vec![Qualifier::new("gene", name)],
        };
        SequenceDoc::new(
            "t",
            Arc::new(seq),
            vec![
                gene(100, 200, "a"),
                gene(300, 400, "b"),
                gene(150, 350, "c"),
            ],
            false,
        )
    }

    fn spans(d: &SequenceDoc) -> Vec<Option<(u64, u64)>> {
        (0..3)
            .map(|i| d.feature(FeatureId(i)).and_then(|f| f.location.bounds()))
            .collect()
    }

    #[test]
    fn edits_move_features_and_queries_follow() {
        let mut d = doc();
        assert_eq!(d.features_in(250, 260), vec![FeatureId(2)]);
        d.insert(50, b"GGGGG");
        assert_eq!(
            spans(&d),
            vec![Some((105, 205)), Some((305, 405)), Some((155, 355))]
        );
        assert_eq!(d.features_in(250, 260), vec![FeatureId(2)]);
        d.delete(290, 420);
        assert_eq!(spans(&d), vec![Some((105, 205)), None, Some((155, 290))]);
        assert_eq!(d.len(), 1005 - 130);
    }

    #[test]
    fn undo_restores_exactly() {
        let mut d = doc();
        let original = spans(&d);
        let seq = d.read(0, d.len());
        let r = d.delete(120, 320);
        // Undo: re-insert, then put overlapping features back.
        d.insert(r.change.pos, &r.removed);
        for (id, f) in r.before {
            d.restore_feature(id, Some(f));
        }
        assert_eq!(spans(&d), original);
        assert_eq!(d.read(0, d.len()), seq);
    }

    #[test]
    fn changes_since_maps_old_positions() {
        let mut d = doc();
        let v0 = d.version();
        d.insert(0, b"AA");
        d.delete(10, 15);
        let changes: Vec<Change> = d.changes_since(v0).copied().collect();
        assert_eq!(
            crate::coord::map_through(&changes, 100, crate::coord::Bias::Left),
            97
        );
    }

    #[test]
    fn random_edits_undo_to_the_original() {
        let mut x = 0x9E37_79B9u64;
        let mut rnd = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n.max(1)
        };
        let mut d = doc();
        for i in 0..40 {
            let s = rnd(900);
            let mut loc = Location::simple(s, s + 1 + rnd(80), Strand::Reverse);
            loc.segments.push(basecraft_protocol::Segment::new(
                s + 100,
                s + 150,
                Strand::Reverse,
            ));
            d.add_feature(Feature {
                kind: "CDS".into(),
                location: loc,
                qualifiers: vec![Qualifier::new("n", i.to_string())],
            });
        }
        let snapshot = |d: &SequenceDoc| {
            (
                d.read(0, d.len()),
                d.features()
                    .map(|(i, f)| (i, f.clone()))
                    .collect::<Vec<_>>(),
            )
        };
        let original = snapshot(&d);
        let mut undo = Vec::new();
        for _ in 0..300 {
            let len = d.len();
            let start = rnd(len);
            let end = (start + rnd(30)).min(len);
            let ins: Vec<u8> = (0..rnd(8)).map(|k| b"ACGT"[(k % 4) as usize]).collect();
            undo.push((ins.len() as u64, d.replace(start, end, &ins)));
        }
        for (inserted, r) in undo.into_iter().rev() {
            d.replace(r.change.pos, r.change.pos + inserted, &r.removed);
            for (id, f) in r.before {
                d.restore_feature(id, Some(f));
            }
        }
        assert_eq!(snapshot(&d), original);
    }
}
