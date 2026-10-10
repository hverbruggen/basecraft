//! A session: open documents, the operations applied to them, undo and redo,
//! and the log.
//!
//! Every change goes through [`Session::apply`], [`Session::undo`] or
//! [`Session::redo`], so the log is complete. Consecutive single-place
//! insertions by the same actor within a second merge into one operation,
//! like an editor's undo grouping, so a burst of typing is one log entry.
//! Alignment edits merge the same way: typing over residues along a row,
//! gaps inserted one after another, a block slid step by step.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use basecraft_docs::alignment::Undo as AlnUndo;
use basecraft_docs::{AlignmentDoc, EditResult, FeatureId, SequenceDoc};
use basecraft_protocol::ops::LOG_VERSION;
use basecraft_protocol::{Actor, DocState, Feature, LogEntry, Op, OpResult};

use crate::error::invalid;
use crate::files::{self, HashingWriter, Loaded, Opened, OpenedAlignment};
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
    Alignment(Vec<AlnUndo>),
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
    content: Loaded,
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

    /// A sequence document. Panics if `id` is an alignment.
    pub fn doc(&self, id: DocId) -> &SequenceDoc {
        match &self.docs[id.0].content {
            Loaded::Sequence(o) => &o.doc,
            Loaded::Alignment(_) => panic!("document {} is an alignment", id.0),
        }
    }

    /// An alignment document. Panics if `id` is a sequence.
    pub fn alignment(&self, id: DocId) -> &AlignmentDoc {
        match &self.docs[id.0].content {
            Loaded::Alignment(o) => &o.doc,
            Loaded::Sequence(_) => panic!("document {} is a sequence", id.0),
        }
    }

    pub fn is_alignment(&self, id: DocId) -> bool {
        matches!(self.docs[id.0].content, Loaded::Alignment(_))
    }

    fn sequence_of(&self, id: DocId) -> Result<&Opened> {
        match &self.docs[id.0].content {
            Loaded::Sequence(o) => Ok(o),
            Loaded::Alignment(_) => Err(invalid("this needs a sequence, not an alignment")),
        }
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

    /// The fingerprint the log records after each operation.
    pub fn state(&self, id: DocId) -> DocState {
        match &self.docs[id.0].content {
            Loaded::Sequence(o) => DocState {
                len: o.doc.len(),
                features: o.doc.feature_count() as u64,
                rows: None,
            },
            Loaded::Alignment(o) => DocState {
                len: o.doc.width(),
                features: 0,
                rows: Some(o.doc.row_count() as u64),
            },
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
        let content = files::load(path)?;
        let key = self.relative(path);
        let mut hashes = vec![(key.clone(), basecraft_io::cache::content_hash(path)?)];
        let sidecar = files::gff_sidecar(path);
        if matches!(&content, Loaded::Sequence(o) if o.format == files::Format::Fasta)
            && sidecar.exists()
        {
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
            content,
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
        let format = files::Format::from_path(&target)?;
        let mut hashes = Vec::new();
        let mut main_hash = String::new();
        let opened = match &self.docs[id.0].content {
            Loaded::Alignment(a) => {
                if format != a.format {
                    return Err(invalid("saving to a different format isn't supported yet"));
                }
                basecraft_io::write_atomic(&target, |w| {
                    let mut hw = HashingWriter::new(w);
                    files::write_alignment(&a.doc, &mut hw)?;
                    main_hash = hw.hash();
                    Ok(())
                })?;
                hashes.push((self.relative(&target), main_hash));
                return self.log_save(id, &target, hashes);
            }
            Loaded::Sequence(o) => o,
        };
        if format != opened.format {
            return Err(invalid("saving to a different format isn't supported yet"));
        }
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
        self.log_save(id, &target, hashes)
    }

    fn log_save(&mut self, id: DocId, target: &Path, hashes: Vec<(String, String)>) -> Result<()> {
        let key = self.relative(target);
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
        let doc = &self.sequence_of(id)?.doc;
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

    /// Log a plugin run that read and wrote files, such as an aligner. Its
    /// `inputs` come with the hashes of exactly what the tool was given;
    /// the outputs are hashed here. The run changes no open document.
    pub fn log_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        (plugin, operation): (&str, &str),
        params: serde_json::Value,
        inputs: &[(String, String)],
        outputs: &[PathBuf],
    ) -> Result<()> {
        self.flush()?;
        let mut hashes = inputs.to_vec();
        let mut keys = Vec::new();
        for path in outputs {
            let key = self.relative(path);
            hashes.push((key.clone(), basecraft_io::hash_file(path)?));
            keys.push(key);
        }
        let entry = self.entry(
            id,
            actor,
            Op::ToolRun {
                plugin: plugin.to_string(),
                operation: operation.to_string(),
                params,
                inputs: inputs.iter().map(|(k, _)| k.clone()).collect(),
                outputs: keys,
            },
            OpResult {
                hashes,
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
        let rec = files::extract_record(&self.sequence_of(id)?.doc, start, end, segments, reverse);
        let mut hw = HashingWriter::new(std::io::sink());
        basecraft_io::genbank::write(&mut hw, &rec)?;
        Ok(hw.hash())
    }

    /// Hashes the files a save would write, without writing them.
    pub fn save_hashes(&self, id: DocId) -> Result<Vec<String>> {
        let opened = match &self.docs[id.0].content {
            Loaded::Sequence(o) => o,
            Loaded::Alignment(a) => {
                let mut hw = HashingWriter::new(std::io::sink());
                files::write_alignment(&a.doc, &mut hw)?;
                return Ok(vec![hw.hash()]);
            }
        };
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
        let mergeable = mergeable(&op);
        slot.undo.push(Applied {
            entry_id: entry_id.clone(),
            op,
            inverse,
            group: self.group,
        });
        if mergeable && self.merge {
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
        if let Some(p) = &self.pending
            && p.doc == doc
            && p.entry.actor == *actor
            && p.last.elapsed() < MERGE_WINDOW
            && let Some(merged) = merge_alignment(&p.entry.op, op)
        {
            let (Inverse::Alignment(steps), _) = self.execute(doc, op)? else {
                return Err(invalid("alignment edit without alignment undo"));
            };
            let state = self.state(doc);
            let slot = &mut self.docs[doc.0];
            slot.redo.clear();
            let top = slot
                .undo
                .last_mut()
                .ok_or_else(|| invalid("pending burst without undo entry"))?;
            if let Inverse::Alignment(first) = &mut top.inverse {
                first.extend(steps);
            }
            top.op = merged.clone();
            if let Some(p) = &mut self.pending {
                p.entry.op = merged;
                p.entry.state = state;
                p.last = Instant::now();
            }
            return Ok(true);
        }
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
        let Loaded::Sequence(opened) = &mut self.docs[doc.0].content else {
            return Ok(false);
        };
        let result = opened.doc.insert(*start, residues.as_bytes());
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
        let doc = match &mut self.docs[id.0].content {
            Loaded::Sequence(o) => &mut o.doc,
            Loaded::Alignment(a) => {
                return execute_alignment(a, op).map(|u| (Inverse::Alignment(u), None));
            }
        };
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
            Op::AlnReplace { .. }
            | Op::AlnInsert { .. }
            | Op::AlnInsertGaps { .. }
            | Op::AlnDelete { .. }
            | Op::AlnShift { .. }
            | Op::AlnRemoveGapColumns {}
            | Op::AlnMoveRows { .. }
            | Op::AlnRenameRow { .. }
            | Op::AlnRemoveRows { .. } => Err(invalid(format!("{} needs an alignment", op.name()))),
            Op::Open { .. }
            | Op::Save { .. }
            | Op::Extract { .. }
            | Op::ToolRun { .. }
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
        match (&mut self.docs[id.0].content, &applied.inverse) {
            (Loaded::Alignment(a), Inverse::Alignment(steps)) => a.doc.undo(steps),
            (Loaded::Sequence(o), inverse) => undo_sequence(&mut o.doc, inverse),
            (Loaded::Alignment(_), _) => return Err(invalid("sequence undo on an alignment")),
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
                let Loaded::Sequence(o) = &mut self.docs[id.0].content else {
                    return Err(invalid("feature redo on an alignment"));
                };
                o.doc.restore_feature(*fid, Some(feature.clone()));
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

fn undo_sequence(doc: &mut SequenceDoc, inverse: &Inverse) {
    match inverse {
        Inverse::Residues { inserted, result } => {
            let pos = result.change.pos;
            doc.replace(pos, pos + inserted, &result.removed);
            for (fid, f) in &result.before {
                doc.restore_feature(*fid, Some(f.clone()));
            }
        }
        Inverse::Feature { id: fid, before } => doc.restore_feature(*fid, before.clone()),
        Inverse::Alignment(_) => {}
    }
}

fn execute_alignment(a: &mut OpenedAlignment, op: &Op) -> Result<Vec<AlnUndo>> {
    let doc = &mut a.doc;
    let edit = match op {
        Op::AlnReplace {
            row,
            start,
            end,
            residues,
        } => {
            let residues = normalise(residues)?;
            doc.replace(*row as usize, *start, *end, residues.as_bytes())
        }
        Op::AlnInsert { row, col, residues } => {
            let residues = normalise(residues)?;
            doc.insert(*row as usize, *col, residues.as_bytes())
        }
        Op::AlnInsertGaps { rows, col, count } => doc.insert_gaps(rows, *col, *count),
        Op::AlnDelete { rows, start, end } => doc.delete(rows, *start, *end),
        Op::AlnShift {
            rows,
            start,
            end,
            by,
        } => doc.shift(rows, *start, *end, *by),
        Op::AlnRemoveGapColumns {} => Ok(doc.remove_gap_columns().0),
        Op::AlnMoveRows { rows, to } => doc.move_rows(rows, *to),
        Op::AlnRenameRow { row, name } => doc.rename(*row as usize, name),
        Op::AlnRemoveRows { rows } => doc.remove_rows(rows),
        _ => return Err(invalid(format!("{} needs a sequence", op.name()))),
    };
    edit.map_err(|e| invalid(e.0))
}

/// Edits whose log entries can grow into a burst.
fn mergeable(op: &Op) -> bool {
    match op {
        Op::SeqReplace {
            start,
            end,
            residues,
        } => start == end && !residues.is_empty(),
        Op::AlnReplace {
            start,
            end,
            residues,
            ..
        } => end - start == residues.len() as u64,
        Op::AlnInsert { .. } | Op::AlnInsertGaps { .. } | Op::AlnShift { .. } => true,
        _ => false,
    }
}

/// One alignment edit that does what `prev` then `next` do, if there is one:
/// typing over residues along a row, gaps inserted after the previous ones,
/// or a block slid on from where it was left.
fn merge_alignment(prev: &Op, next: &Op) -> Option<Op> {
    match (prev, next) {
        (
            Op::AlnReplace {
                row,
                start,
                end,
                residues,
            },
            Op::AlnReplace {
                row: row2,
                start: start2,
                end: end2,
                residues: residues2,
            },
        ) if row == row2
            && end - start == residues.len() as u64
            && end2 - start2 == residues2.len() as u64
            && start2 == end =>
        {
            Some(Op::AlnReplace {
                row: *row,
                start: *start,
                end: *end2,
                residues: format!("{residues}{residues2}"),
            })
        }
        (
            Op::AlnInsert { row, col, residues },
            Op::AlnInsert {
                row: row2,
                col: col2,
                residues: residues2,
            },
        ) if row == row2 && *col2 == col + residues.len() as u64 => Some(Op::AlnInsert {
            row: *row,
            col: *col,
            residues: format!("{residues}{residues2}"),
        }),
        (
            Op::AlnInsertGaps { rows, col, count },
            Op::AlnInsertGaps {
                rows: rows2,
                col: col2,
                count: count2,
            },
        ) if rows == rows2 && *col2 == col + count => Some(Op::AlnInsertGaps {
            rows: rows.clone(),
            col: *col,
            count: count + count2,
        }),
        (
            Op::AlnShift {
                rows,
                start,
                end,
                by,
            },
            Op::AlnShift {
                rows: rows2,
                start: start2,
                end: end2,
                by: by2,
            },
        ) if rows == rows2
            && start.checked_add_signed(*by) == Some(*start2)
            && end.checked_add_signed(*by) == Some(*end2) =>
        {
            Some(Op::AlnShift {
                rows: rows.clone(),
                start: *start,
                end: *end,
                by: by + by2,
            })
        }
        _ => None,
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
