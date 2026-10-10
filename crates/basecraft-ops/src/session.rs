//! A session: open documents, the operations applied to them, undo and redo,
//! and the log.
//!
//! Every change goes through [`Session::apply`], [`Session::undo`] or
//! [`Session::redo`], so the log is complete. Consecutive single-place
//! insertions by the same actor within a second merge into one operation,
//! like an editor's undo grouping, so a burst of typing is one log entry.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use basecraft_docs::{EditResult, FeatureId, SequenceDoc};
use basecraft_protocol::ops::LOG_VERSION;
use basecraft_protocol::{Actor, DocState, Feature, LogEntry, Op, OpResult};

use crate::error::invalid;
use crate::files::{self, HashingWriter, Opened};
use crate::log::LogWriter;
use crate::{Error, Result};

/// Typing within this interval, at the end of the previous insertion,
/// merges into one operation.
const MERGE_WINDOW: Duration = Duration::from_secs(1);

/// Files up to this size are copied into the project's object store when
/// opened, so replay works after the original is overwritten.
pub const OBJECT_STORE_LIMIT: u64 = 100 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocId(pub usize);

enum Inverse {
    Residues {
        inserted: u64,
        result: EditResult,
    },
    Feature {
        id: FeatureId,
        before: Option<Feature>,
    },
}

struct Applied {
    entry_id: String,
    op: Op,
    inverse: Inverse,
    /// Operations applied together (a plugin's results) share a group and
    /// undo and redo together.
    group: Option<u64>,
}

struct Slot {
    key: String,
    opened: Opened,
    undo: Vec<Applied>,
    redo: Vec<Applied>,
}

/// A merged typing burst whose log entry hasn't been written yet.
struct Pending {
    doc: DocId,
    entry: LogEntry,
    last: Instant,
}

/// What `apply` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub entry_id: String,
    pub feature: Option<FeatureId>,
}

pub struct Session {
    root: PathBuf,
    id: String,
    host: String,
    actor: Actor,
    log: Option<LogWriter>,
    docs: Vec<Slot>,
    pending: Option<Pending>,
    store_objects: bool,
    /// Merge typing bursts. Off for replay, where each logged entry must
    /// stay one operation.
    merge: bool,
    /// The open undo group, if any, and the next group number.
    group: Option<u64>,
    next_group: u64,
    /// Monotonic, so entries made within one millisecond still sort in order.
    ids: Mutex<ulid::Generator>,
}

impl Session {
    /// A logging session for the project at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let host = basecraft_platform::hostname();
        let log = Some(LogWriter::new(&root, &host));
        Self {
            root,
            id: ulid::Ulid::generate().to_string(),
            host,
            actor: Actor::Human {
                user: basecraft_platform::username(),
            },
            log,
            docs: Vec::new(),
            pending: None,
            store_objects: true,
            merge: true,
            group: None,
            next_group: 1,
            ids: Mutex::new(ulid::Generator::new()),
        }
    }

    /// A session that logs nothing, for replay and tests.
    pub fn unlogged(root: impl Into<PathBuf>) -> Self {
        let mut s = Self::new(root);
        s.log = None;
        s.store_objects = false;
        s.merge = false;
        s
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn set_actor(&mut self, actor: Actor) {
        self.actor = actor;
    }

    pub fn doc(&self, id: DocId) -> &SequenceDoc {
        &self.docs[id.0].opened.doc
    }

    pub fn doc_key(&self, id: DocId) -> &str {
        &self.docs[id.0].key
    }

    pub fn can_undo(&self, id: DocId) -> bool {
        !self.docs[id.0].undo.is_empty()
    }

    pub fn can_redo(&self, id: DocId) -> bool {
        !self.docs[id.0].redo.is_empty()
    }

    /// Project-relative path, or the absolute path for files outside it.
    fn relative(&self, path: &Path) -> String {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        abs.strip_prefix(&root).map_or_else(
            |_| abs.to_string_lossy().into_owned(),
            |p| p.to_string_lossy().into_owned(),
        )
    }

    fn resolve(&self, key: &str) -> PathBuf {
        let p = Path::new(key);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    fn state(&self, id: DocId) -> DocState {
        let doc = self.doc(id);
        DocState {
            len: doc.len(),
            features: doc.feature_count() as u64,
        }
    }

    fn entry(&self, doc: DocId, actor: Actor, op: Op, result: OpResult) -> LogEntry {
        LogEntry {
            v: LOG_VERSION,
            id: self.next_id(),
            t: basecraft_platform::now_rfc3339(),
            host: self.host.clone(),
            session: self.id.clone(),
            actor,
            doc: self.docs[doc.0].key.clone(),
            op,
            result,
            state: self.state(doc),
        }
    }

    fn next_id(&self) -> String {
        let mut g = self
            .ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        g.generate()
            .unwrap_or_else(|_| ulid::Ulid::generate())
            .to_string()
    }

    fn write(&mut self, entry: &LogEntry) -> Result<()> {
        if let Some(log) = &mut self.log {
            log.append(entry)?;
        }
        Ok(())
    }

    /// Write out a pending typing burst.
    pub fn flush(&mut self) -> Result<()> {
        if let Some(p) = self.pending.take() {
            self.write(&p.entry)?;
        }
        Ok(())
    }

    // Open and save.

    pub fn open(&mut self, path: &Path) -> Result<DocId> {
        self.flush()?;
        let opened = files::open(path)?;
        let key = self.relative(path);
        let mut hashes = vec![(key.clone(), basecraft_io::cache::content_hash(path)?)];
        let sidecar = files::gff_sidecar(path);
        if opened.format == files::Format::Fasta && sidecar.exists() {
            hashes.push((
                self.relative(&sidecar),
                basecraft_io::cache::content_hash(&sidecar)?,
            ));
        }
        if self.store_objects {
            for (rel, hash) in &hashes {
                self.store_object(&self.resolve(rel), hash)?;
            }
        }
        self.docs.push(Slot {
            key: key.clone(),
            opened,
            undo: Vec::new(),
            redo: Vec::new(),
        });
        let id = DocId(self.docs.len() - 1);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Open { path: key },
            OpResult {
                hashes,
                feature: None,
            },
        );
        self.write(&entry)?;
        Ok(id)
    }

    fn store_object(&self, path: &Path, hash: &str) -> Result<()> {
        let size = std::fs::metadata(path)?.len();
        if size > OBJECT_STORE_LIMIT {
            return Ok(());
        }
        let dest = objects_dir(&self.root).join(hash.trim_start_matches("b3:"));
        if !dest.exists() {
            std::fs::create_dir_all(objects_dir(&self.root))?;
            let bytes = std::fs::read(path)?;
            basecraft_io::write_atomic(&dest, |w| w.write_all(&bytes))?;
        }
        Ok(())
    }

    /// Save a document to its own path, or to `path`.
    pub fn save(&mut self, id: DocId, path: Option<&Path>) -> Result<()> {
        self.flush()?;
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
        let opened = &self.docs[id.0].opened;
        let format = files::Format::from_path(&target)?;
        if format != opened.format {
            return Err(invalid("saving to a different format isn't supported yet"));
        }
        let mut hashes = Vec::new();
        let mut main_hash = String::new();
        basecraft_io::write_atomic(&target, |w| {
            let mut hw = HashingWriter::new(w);
            files::write_main(opened, &mut hw)?;
            main_hash = hw.hash();
            Ok(())
        })?;
        hashes.push((self.relative(&target), main_hash));
        let sidecar = files::gff_sidecar(&target);
        let mut side_hash = None;
        if format == files::Format::Fasta {
            basecraft_io::write_atomic(&sidecar, |w| {
                let mut hw = HashingWriter::new(w);
                if let Some(r) = files::write_sidecar(opened, &mut hw) {
                    r?;
                }
                side_hash = Some(hw.hash());
                Ok(())
            })?;
        }
        if let Some(h) = side_hash {
            hashes.push((self.relative(&sidecar), h));
        }
        let key = self.relative(&target);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Save { path: key },
            OpResult {
                hashes,
                feature: None,
            },
        );
        self.write(&entry)
    }

    /// Extract residues `start..end` with their annotations to a new
    /// GenBank file at `path`, and log it.
    pub fn extract(
        &mut self,
        id: DocId,
        (start, end): (u64, u64),
        segments: &[(u64, u64)],
        reverse: bool,
        path: &Path,
    ) -> Result<()> {
        self.flush()?;
        let doc = &self.docs[id.0].opened.doc;
        if start >= end || end > doc.len() {
            return Err(invalid(format!(
                "range {start}..{end} outside 0..{}",
                doc.len()
            )));
        }
        let rec = files::extract_record(doc, start, end, segments, reverse);
        let mut hash = String::new();
        basecraft_io::write_atomic(path, |w| {
            let mut hw = HashingWriter::new(w);
            basecraft_io::genbank::write(&mut hw, &rec)?;
            hash = hw.hash();
            Ok(())
        })?;
        let key = self.relative(path);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Extract {
                start,
                end,
                path: key.clone(),
                segments: segments.to_vec(),
                reverse,
            },
            OpResult {
                hashes: vec![(key, hash)],
                feature: None,
            },
        );
        self.write(&entry)
    }

    /// The hash of the file an extract would write, without writing it.
    pub fn extract_hash(
        &self,
        id: DocId,
        (start, end): (u64, u64),
        segments: &[(u64, u64)],
        reverse: bool,
    ) -> Result<String> {
        let rec = files::extract_record(&self.docs[id.0].opened.doc, start, end, segments, reverse);
        let mut hw = HashingWriter::new(std::io::sink());
        basecraft_io::genbank::write(&mut hw, &rec)?;
        Ok(hw.hash())
    }

    /// Hashes the files a save would write, without writing them.
    pub fn save_hashes(&self, id: DocId) -> Result<Vec<String>> {
        let opened = &self.docs[id.0].opened;
        let mut hw = HashingWriter::new(std::io::sink());
        files::write_main(opened, &mut hw)?;
        let mut out = vec![hw.hash()];
        let mut hw = HashingWriter::new(std::io::sink());
        if let Some(r) = files::write_sidecar(opened, &mut hw) {
            r?;
            out.push(hw.hash());
        }
        Ok(out)
    }

    // Operations.

    /// Apply an edit as the session's default actor.
    pub fn apply(&mut self, doc: DocId, op: Op) -> Result<Outcome> {
        let actor = self.actor.clone();
        self.apply_as(doc, op, actor)
    }

    pub fn apply_as(&mut self, doc: DocId, op: Op, actor: Actor) -> Result<Outcome> {
        if self.merge && self.try_merge(doc, &op, &actor)? {
            let id = self
                .pending
                .as_ref()
                .map(|p| p.entry.id.clone())
                .unwrap_or_default();
            return Ok(Outcome {
                entry_id: id,
                feature: None,
            });
        }
        self.flush()?;
        let (inverse, feature) = self.execute(doc, &op)?;
        let result = OpResult {
            hashes: Vec::new(),
            feature: feature.map(|f| f.0),
        };
        let entry = self.entry(doc, actor, op.clone(), result);
        let entry_id = entry.id.clone();
        let slot = &mut self.docs[doc.0];
        slot.redo.clear();
        let is_insert = matches!(&op, Op::SeqReplace { start, end, residues } if start == end && !residues.is_empty());
        slot.undo.push(Applied {
            entry_id: entry_id.clone(),
            op,
            inverse,
            group: self.group,
        });
        if is_insert && self.merge {
            self.pending = Some(Pending {
                doc,
                entry,
                last: Instant::now(),
            });
        } else {
            self.write(&entry)?;
        }
        Ok(Outcome { entry_id, feature })
    }

    /// Merge an insertion into the pending burst if it continues it.
    fn try_merge(&mut self, doc: DocId, op: &Op, actor: &Actor) -> Result<bool> {
        let Op::SeqReplace {
            start,
            end,
            residues,
        } = op
        else {
            return Ok(false);
        };
        let Some(p) = &self.pending else {
            return Ok(false);
        };
        let Op::SeqReplace {
            start: p_start,
            residues: p_res,
            ..
        } = &p.entry.op
        else {
            return Ok(false);
        };
        let continues = start == end
            && !residues.is_empty()
            && p.doc == doc
            && p.entry.actor == *actor
            && *start == p_start + p_res.len() as u64
            && p.last.elapsed() < MERGE_WINDOW;
        if !continues {
            return Ok(false);
        }
        let residues = normalise(residues)?;
        let result = self.docs[doc.0]
            .opened
            .doc
            .insert(*start, residues.as_bytes());
        let state = self.state(doc);
        let slot = &mut self.docs[doc.0];
        slot.redo.clear();
        let top = slot
            .undo
            .last_mut()
            .ok_or_else(|| invalid("pending burst without undo entry"))?;
        if let (
            Op::SeqReplace { residues: r, .. },
            Inverse::Residues {
                inserted,
                result: first,
            },
        ) = (&mut top.op, &mut top.inverse)
        {
            r.push_str(&residues);
            *inserted += residues.len() as u64;
            // Keep the pre-burst state of each feature; add features the
            // burst touched later.
            for (id, f) in result.before {
                if !first.before.iter().any(|(i, _)| *i == id) {
                    first.before.push((id, f));
                }
            }
        }
        if let Some(p) = &mut self.pending {
            if let Op::SeqReplace { residues: r, .. } = &mut p.entry.op {
                r.push_str(&residues);
            }
            p.entry.state = state;
            p.last = Instant::now();
        }
        Ok(true)
    }

    /// Carry out an edit on the document; returns its inverse.
    fn execute(&mut self, id: DocId, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        let doc = &mut self.docs[id.0].opened.doc;
        match op {
            Op::SeqReplace {
                start,
                end,
                residues,
            } => {
                if start > end || *end > doc.len() {
                    return Err(invalid(format!(
                        "range {start}..{end} outside 0..{}",
                        doc.len()
                    )));
                }
                let residues = normalise(residues)?;
                let result = doc.replace(*start, *end, residues.as_bytes());
                Ok((
                    Inverse::Residues {
                        inserted: residues.len() as u64,
                        result,
                    },
                    None,
                ))
            }
            Op::FeatureAdd { feature } => {
                check_location(feature, doc.len())?;
                let fid = doc.add_feature(feature.clone());
                Ok((
                    Inverse::Feature {
                        id: fid,
                        before: None,
                    },
                    Some(fid),
                ))
            }
            Op::FeatureRemove { id } => {
                let fid = FeatureId(*id);
                let before = doc
                    .remove_feature(fid)
                    .ok_or_else(|| invalid(format!("no feature {id}")))?;
                Ok((
                    Inverse::Feature {
                        id: fid,
                        before: Some(before),
                    },
                    None,
                ))
            }
            Op::FeatureUpdate { id, feature } => {
                check_location(feature, doc.len())?;
                let fid = FeatureId(*id);
                if doc.feature(fid).is_none() {
                    return Err(invalid(format!("no feature {id}")));
                }
                let before = doc.update_feature(fid, feature.clone());
                Ok((Inverse::Feature { id: fid, before }, None))
            }
            Op::Open { .. }
            | Op::Save { .. }
            | Op::Extract { .. }
            | Op::Undo { .. }
            | Op::Redo { .. } => Err(invalid(format!("{} is not an edit", op.name()))),
        }
    }

    /// Undo the last operation on a document; returns the undone entry's id.
    /// Start an undo group: operations applied until [`Session::end_group`]
    /// undo and redo as one step. Each is still logged on its own, so the
    /// log replays exactly.
    pub fn begin_group(&mut self) {
        self.group = Some(self.next_group);
        self.next_group += 1;
    }

    pub fn end_group(&mut self) {
        self.group = None;
    }

    /// Undo the last step on a document (one operation, or a whole group);
    /// returns the id of the last entry undone.
    pub fn undo(&mut self, id: DocId) -> Result<String> {
        let group = self.docs[id.0].undo.last().and_then(|a| a.group);
        let mut last = self.undo_one(id)?;
        while group.is_some() && self.docs[id.0].undo.last().and_then(|a| a.group) == group {
            last = self.undo_one(id)?;
        }
        Ok(last)
    }

    /// Redo the last undone step (one operation, or a whole group).
    pub fn redo(&mut self, id: DocId) -> Result<String> {
        let group = self.docs[id.0].redo.last().and_then(|a| a.group);
        let mut last = self.redo_one(id)?;
        while group.is_some() && self.docs[id.0].redo.last().and_then(|a| a.group) == group {
            last = self.redo_one(id)?;
        }
        Ok(last)
    }

    /// Undo exactly one operation, ignoring groups. Replay uses this: the
    /// log holds one undo entry per operation.
    pub fn undo_single(&mut self, id: DocId) -> Result<String> {
        self.undo_one(id)
    }

    pub fn redo_single(&mut self, id: DocId) -> Result<String> {
        self.redo_one(id)
    }

    fn undo_one(&mut self, id: DocId) -> Result<String> {
        self.flush()?;
        let applied = self.docs[id.0].undo.pop().ok_or(Error::NothingTo("undo"))?;
        let doc = &mut self.docs[id.0].opened.doc;
        match &applied.inverse {
            Inverse::Residues { inserted, result } => {
                let pos = result.change.pos;
                doc.replace(pos, pos + inserted, &result.removed);
                for (fid, f) in &result.before {
                    doc.restore_feature(*fid, Some(f.clone()));
                }
            }
            Inverse::Feature { id: fid, before } => doc.restore_feature(*fid, before.clone()),
        }
        let of = applied.entry_id.clone();
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Undo { of: of.clone() },
            OpResult::default(),
        );
        self.docs[id.0].redo.push(applied);
        self.write(&entry)?;
        Ok(of)
    }

    fn redo_one(&mut self, id: DocId) -> Result<String> {
        self.flush()?;
        let applied = self.docs[id.0].redo.pop().ok_or(Error::NothingTo("redo"))?;
        let inverse = match (&applied.op, &applied.inverse) {
            // A re-added feature keeps its id.
            (Op::FeatureAdd { feature }, Inverse::Feature { id: fid, .. }) => {
                self.docs[id.0]
                    .opened
                    .doc
                    .restore_feature(*fid, Some(feature.clone()));
                Inverse::Feature {
                    id: *fid,
                    before: None,
                }
            }
            (op, _) => self.execute(id, &op.clone())?.0,
        };
        let of = applied.entry_id.clone();
        self.docs[id.0].undo.push(Applied { inverse, ..applied });
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Redo { of: of.clone() },
            OpResult::default(),
        );
        self.write(&entry)?;
        Ok(of)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

pub fn objects_dir(root: &Path) -> PathBuf {
    root.join(".basecraft").join("objects")
}

/// Upper-case residues; reject anything that isn't a letter, `-` or `*`.
fn normalise(residues: &str) -> Result<String> {
    if let Some(c) = residues
        .chars()
        .find(|c| !(c.is_ascii_alphabetic() || *c == '-' || *c == '*'))
    {
        return Err(invalid(format!("{c:?} is not a base or residue code")));
    }
    Ok(residues.to_ascii_uppercase())
}

fn check_location(f: &Feature, len: u64) -> Result<()> {
    if f.location.segments.is_empty() {
        return Err(invalid("feature has no segments"));
    }
    for s in &f.location.segments {
        if s.start > s.end || s.end > len {
            return Err(invalid(format!(
                "segment {}..{} outside 0..{len}",
                s.start, s.end
            )));
        }
    }
    Ok(())
}
