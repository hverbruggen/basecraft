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
use basecraft_docs::{AlignmentDoc, EditResult, FeatureId, SequenceDoc, TreeDoc};
use basecraft_protocol::ops::LOG_VERSION;
use basecraft_protocol::{Actor, DocState, Feature, LogEntry, Op, OpResult};

use crate::error::invalid;
use crate::files::{self, HashingWriter, Loaded, Opened, OpenedAlignment, OpenedTree};
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
    /// The whole tree as it was: trees are small.
    Tree(Box<TreeDoc>),
    Delimitation(Box<basecraft_protocol::delimitation::DelimitationWorkspace>),
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
    generation: u64,
    undo: Vec<Applied>,
    redo: Vec<Applied>,
    baseline: Option<LogEntry>,
    baseline_logged: bool,
    accepted_hashes: Vec<(String, String)>,
    saved_state_hashes: Vec<String>,
    absent_sidecars: Vec<String>,
    origin: Option<basecraft_protocol::provenance::Publication>,
    origin_issue: Option<String>,
    origin_bundle: PathBuf,
    disk_stamp: basecraft_protocol::provenance::FileStamp,
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
    storage_root: PathBuf,
    provenance_policy: basecraft_platform::settings::ProvenancePolicy,
    local_provenance_root: PathBuf,
    tracked: bool,
    memory_objects: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    events: Vec<LogEntry>,
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
            storage_root: root.clone(),
            provenance_policy: basecraft_platform::settings::ProvenancePolicy::Portable,
            local_provenance_root: root.join(".basecraft/provenance"),
            tracked: true,
            memory_objects: Mutex::new(std::collections::HashMap::new()),
            events: Vec::new(),
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

    /// A viewing session whose first edit starts recovery in application state,
    /// never next to the source. Discovery creates no directories.
    pub fn recoverable(root: impl Into<PathBuf>) -> Result<Self> {
        let paths =
            basecraft_platform::paths::AppPaths::discover().map_err(|e| invalid(e.to_string()))?;
        Self::recoverable_with_paths(root, &paths)
    }

    pub fn recoverable_with_paths(
        root: impl Into<PathBuf>,
        paths: &basecraft_platform::paths::AppPaths,
    ) -> Result<Self> {
        let preferences = basecraft_platform::settings::SettingsStore::new(paths.preferences())
            .load()
            .map_err(|e| invalid(e.to_string()))?;
        let mut session = Self::new(root);
        session.storage_root = paths.state.join("recovery").join(&session.id);
        session.local_provenance_root = paths.state.join("provenance");
        session.provenance_policy = preferences.files.provenance;
        session.log = preferences
            .files
            .recovery
            .then(|| LogWriter::new(&session.storage_root, &session.id));
        session.store_objects = preferences.files.recovery;
        // Each accepted edit must already be recoverable. Deferred typing-burst
        // serialization is retained only for explicitly constructed legacy sessions.
        session.merge = false;
        Ok(session)
    }

    pub fn recovery_root(&self) -> &Path {
        &self.storage_root
    }

    /// A session that logs nothing, for replay and tests.
    pub fn unlogged(root: impl Into<PathBuf>) -> Self {
        let mut s = Self::new(root);
        s.log = None;
        s.store_objects = false;
        s.merge = false;
        s.tracked = false;
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
            _ => panic!("document {} isn't a sequence", id.0),
        }
    }

    /// An alignment document. Panics if `id` is a sequence.
    pub fn alignment(&self, id: DocId) -> &AlignmentDoc {
        match &self.docs[id.0].content {
            Loaded::Alignment(o) => &o.doc,
            _ => panic!("document {} isn't an alignment", id.0),
        }
    }

    /// A tree document. Panics if `id` isn't a tree.
    pub fn tree(&self, id: DocId) -> &TreeDoc {
        match &self.docs[id.0].content {
            Loaded::Tree(o) => &o.doc,
            _ => panic!("document {} isn't a tree", id.0),
        }
    }

    pub fn delimitation(
        &self,
        id: DocId,
    ) -> &basecraft_protocol::delimitation::DelimitationWorkspace {
        match &self.docs[id.0].content {
            Loaded::Delimitation(work) => work,
            _ => panic!("not a delimitation workspace"),
        }
    }

    pub fn is_delimitation(&self, id: DocId) -> bool {
        matches!(self.docs[id.0].content, Loaded::Delimitation(_))
    }

    pub(crate) fn restore_replay_paths(&mut self, id: DocId, from: &Path, to: &Path) {
        if let Loaded::Delimitation(work) = &mut self.docs[id.0].content {
            files::rebase_delimitation_paths(work, from, to);
        }
    }

    pub(crate) fn legacy_delimitation_hashes(
        &self,
        id: DocId,
        target: &Path,
    ) -> Result<Option<Vec<String>>> {
        let Loaded::Delimitation(work) = &self.docs[id.0].content else {
            return Ok(None);
        };
        let mut main = HashingWriter::new(std::io::sink());
        files::write_delimitation(work, files::delimitation_xml(target), &mut main)?;
        let hash = main.hash();
        let mut side = HashingWriter::new(std::io::sink());
        files::write_delimitation_sidecar(work, &hash, &mut side)?;
        Ok(Some(vec![hash, side.hash()]))
    }

    pub fn is_tree(&self, id: DocId) -> bool {
        matches!(self.docs[id.0].content, Loaded::Tree(_))
    }

    pub fn is_alignment(&self, id: DocId) -> bool {
        matches!(self.docs[id.0].content, Loaded::Alignment(_))
    }

    fn sequence_of(&self, id: DocId) -> Result<&Opened> {
        match &self.docs[id.0].content {
            Loaded::Sequence(o) => Ok(o),
            Loaded::Delimitation(_) => Err(invalid("this needs a sequence")),
            Loaded::Alignment(_) => Err(invalid("this needs a sequence, not an alignment")),
            Loaded::Tree(_) => Err(invalid("this needs a sequence, not a tree")),
        }
    }

    /// Checked access for plugin snapshots; never index a wrong-kind/closed slot.
    pub fn checked_sequence(&self, id: DocId) -> Result<&SequenceDoc> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        let Loaded::Sequence(sequence) = &slot.content else {
            return Err(invalid("snapshot requires a sequence document"));
        };
        Ok(&sequence.doc)
    }

    pub fn doc_key(&self, id: DocId) -> &str {
        &self.docs[id.0].key
    }

    pub fn generation(&self, id: DocId) -> u64 {
        self.docs[id.0].generation
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
            Loaded::Delimitation(work) => DocState {
                len: work.specimens.len() as u64,
                features: work.hypotheses.len() as u64,
                rows: None,
            },
            Loaded::Tree(o) => DocState {
                len: o.doc.tip_count() as u64,
                features: o.doc.annotations().len() as u64,
                rows: None,
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
        if self.tracked {
            self.events.push(entry.clone());
        }
        Ok(())
    }

    /// Write out a pending typing burst.
    pub fn flush(&mut self) -> Result<()> {
        if let Some(p) = self.pending.take() {
            if let Err(error) = self.write(&p.entry) {
                self.pending = Some(p);
                return Err(error);
            }
        }
        Ok(())
    }

    // Open and save.

    pub fn open(&mut self, path: &Path) -> Result<DocId> {
        self.flush()?;
        let disk_stamp = crate::input_evidence::stamp(path)?;
        let accepted_hash = basecraft_io::hash_file(path)?;
        let format = files::Format::from_path(path)?;
        let sidecar = if matches!(format, files::Format::Tree | files::Format::Delimitation) {
            files::tree_sidecar(path)
        } else {
            files::gff_sidecar(path)
        };
        let sidecar_hash = if sidecar.exists() {
            Some(basecraft_io::hash_file(&sidecar)?)
        } else {
            None
        };
        let content = files::load(path)?;
        let after_sidecar = if sidecar.exists() {
            Some(basecraft_io::hash_file(&sidecar)?)
        } else {
            None
        };
        if basecraft_io::hash_file(path)? != accepted_hash
            || after_sidecar != sidecar_hash
            || crate::input_evidence::stamp(path)? != disk_stamp
        {
            return Err(invalid(
                "source or sidecar changed while opening; retry with stable files",
            ));
        }
        let key = self.relative(path);
        let mut hashes = vec![(key.clone(), accepted_hash)];
        let absent_sidecars = if sidecar.exists() {
            Vec::new()
        } else {
            vec![self.relative(&sidecar)]
        };
        if matches!(&content, Loaded::Sequence(o) if o.format == files::Format::Fasta)
            || matches!(content, Loaded::Tree(_) | Loaded::Delimitation(_))
        {
            if let Some(hash) = sidecar_hash {
                hashes.push((self.relative(&sidecar), hash));
            }
        }
        let (origin, origin_issue) = crate::document_record::read_origin(path, &hashes);
        self.docs.push(Slot {
            key: key.clone(),
            content,
            generation: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            baseline: None,
            baseline_logged: false,
            accepted_hashes: hashes.clone(),
            saved_state_hashes: Vec::new(),
            absent_sidecars,
            origin,
            origin_issue,
            origin_bundle: crate::provenance::bundle_for_result(path),
            disk_stamp,
        });
        let id = DocId(self.docs.len() - 1);
        self.docs[id.0].saved_state_hashes = self.save_hashes(id)?;
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Open { path: key },
            OpResult {
                hashes,
                feature: None,
            },
        );
        self.docs[id.0].baseline = Some(entry);
        Ok(id)
    }

    /// Promote a read-only load before its first durable action. Validate and
    /// retain the accepted originals before any edit or replacement can occur.
    fn track(&mut self, id: DocId) -> Result<()> {
        let Some(entry) = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?
            .baseline
            .clone()
        else {
            return Ok(());
        };
        if self.tracked {
            for (key, hash) in &entry.result.hashes {
                let source = self.resolve(key);
                if basecraft_io::hash_file(&source)? != *hash {
                    return Err(invalid(
                        "source changed since opening; reopen it before editing",
                    ));
                }
                if self.store_objects {
                    self.store_object(&source, hash)?;
                } else {
                    self.snapshot_file(&source)?;
                }
            }
        }
        if self.log.is_some() {
            let source_root = serde_json::to_value(&self.root)?;
            let baseline = serde_json::to_value(&entry)?;
            let manifest = serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1, "session": self.id,
                "source_root": source_root, "status": "unsaved",
                "baseline": baseline, "object_policy_limit": OBJECT_STORE_LIMIT
            }))?;
            std::fs::create_dir_all(&self.storage_root)?;
            basecraft_io::write_atomic(&self.storage_root.join("recovery.json"), |w| {
                w.write_all(&manifest)
            })?;
        }
        if !self.docs[id.0].baseline_logged {
            self.write(&entry)?;
        }
        self.docs[id.0].baseline = None;
        Ok(())
    }

    /// Cheap change token for readers; saves and job outcomes also invalidate a report.
    pub fn record_revision(&self, id: DocId) -> Result<String> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        Ok(format!(
            "{}:{}:{}:{}",
            self.id,
            id.0,
            slot.generation,
            self.events.len()
        ))
    }

    /// Read-only evidence for this session's exact current revision, including pending edits.
    pub fn document_record(&self, id: DocId) -> Result<crate::document_record::DocumentRecord> {
        use basecraft_protocol::provenance::EditActivity;
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        let mut record = slot.origin.as_ref().map_or_else(
            || crate::document_record::DocumentRecord::empty(&slot.key),
            crate::document_record::DocumentRecord::from_publication,
        );
        if let Some(origin) = &slot.origin {
            record
                .origins
                .push((slot.origin_bundle.clone(), origin.clone()));
        }
        record.document.clone_from(&slot.key);
        record.revision = format!("{}:{}:{}", self.id, id.0, slot.generation);
        record.unsaved = self.save_hashes(id)? != slot.saved_state_hashes;
        if let Some(issue) = &slot.origin_issue {
            record.warnings.push(issue.clone());
        }
        if !slot.baseline_logged {
            if let Some(baseline) = &slot.baseline {
                record.events.push(baseline.clone());
            }
        }
        record
            .events
            .extend(self.events.iter().filter(|e| e.doc == slot.key).cloned());
        if let Some(pending) = &self.pending {
            if pending.doc == id {
                record.events.push(pending.entry.clone());
            }
        }
        record
            .effective
            .extend(slot.undo.iter().map(|a| EditActivity {
                id: a.entry_id.clone(),
                operation: a.op.clone(),
            }));
        record.deduplicate();
        Ok(record)
    }

    fn reference_baseline(&mut self, id: DocId) -> Result<()> {
        if !self.docs[id.0].baseline_logged {
            if let Some(entry) = self.docs[id.0].baseline.clone() {
                self.write(&entry)?;
            }
            self.docs[id.0].baseline_logged = true;
        }
        Ok(())
    }

    /// Use an accepted disk revision only for a clean, unchanged document. This
    /// checks metadata and reuses the verified open hash, never rereads disk bytes.
    pub fn referenced_source(
        &self,
        id: DocId,
        bundle: &Path,
    ) -> Result<Option<basecraft_protocol::provenance::InputReference>> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        if self.save_hashes(id)? != slot.saved_state_hashes {
            return Ok(None);
        }
        let path = self.resolve(&slot.key).canonicalize()?;
        let current = crate::input_evidence::stamp(&path)?;
        if current != slot.disk_stamp {
            return Err(invalid(
                "source changed since opening; reopen it before analysis",
            ));
        }
        let hash = slot
            .accepted_hashes
            .first()
            .ok_or_else(|| invalid("source identity unavailable"))?
            .1
            .clone();
        Ok(Some(basecraft_protocol::provenance::InputReference {
            path: crate::input_evidence::relative(bundle, &path)?,
            stamp: current,
            hash,
            extra: std::collections::BTreeMap::new(),
        }))
    }

    pub fn snapshot_file(&self, path: &Path) -> Result<String> {
        self.snapshot_bytes(&std::fs::read(path)?)
    }

    /// Hash and retain the exact immutable buffer consumed by an analysis.
    pub fn snapshot_bytes(&self, bytes: &[u8]) -> Result<String> {
        let hash = basecraft_io::hash_bytes(bytes);
        if self.tracked && bytes.len() as u64 > OBJECT_STORE_LIMIT {
            return Err(invalid(format!(
                "exact input retention exceeds the {OBJECT_STORE_LIMIT}-byte policy limit"
            )));
        }
        if self.tracked && !self.store_objects {
            self.memory_objects
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(hash.clone())
                .or_insert_with(|| bytes.to_vec());
        }
        if self.store_objects && bytes.len() as u64 <= OBJECT_STORE_LIMIT {
            let dest = objects_dir(&self.storage_root).join(hash.trim_start_matches("b3:"));
            if !dest.exists() {
                std::fs::create_dir_all(objects_dir(&self.storage_root))?;
                basecraft_io::write_atomic(&dest, |w| w.write_all(bytes))?;
            }
        }
        Ok(hash)
    }

    fn store_object(&self, path: &Path, hash: &str) -> Result<()> {
        let size = std::fs::metadata(path)?.len();
        if size > OBJECT_STORE_LIMIT {
            return Err(invalid(format!(
                "original input is {size} bytes; exact recovery retention currently supports at most {OBJECT_STORE_LIMIT} bytes"
            )));
        }
        let dest = objects_dir(&self.storage_root).join(hash.trim_start_matches("b3:"));
        if !dest.exists() {
            std::fs::create_dir_all(objects_dir(&self.storage_root))?;
            let bytes = std::fs::read(path)?;
            if basecraft_io::hash_bytes(&bytes) != hash {
                return Err(invalid("source changed while retaining its snapshot"));
            }
            basecraft_io::write_atomic(&dest, |w| w.write_all(&bytes))?;
        }
        Ok(())
    }

    /// Save a document to its own path, or to `path`.
    pub fn save(&mut self, id: DocId, path: Option<&Path>) -> Result<()> {
        self.save_with_evidence(id, path, crate::input_evidence::SavePolicy::Cancel)
    }

    /// Headless callers choose a policy explicitly; default save returns a structured conflict.
    pub fn save_with_evidence(
        &mut self,
        id: DocId,
        path: Option<&Path>,
        policy: crate::input_evidence::SavePolicy,
    ) -> Result<()> {
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
        if self.log.is_some()
            && path.is_none()
            && self.save_hashes(id)? == self.docs[id.0].saved_state_hashes
        {
            return Ok(());
        }
        // Cooperating Basecraft processes serialize the entire data + sidecar +
        // provenance publication. External programs still require hash checks.
        let _save_lock = if self.tracked {
            Some(crate::save_guard::lock(&target)?)
        } else {
            None
        };
        let _evidence_lock = crate::input_evidence::lock()?;
        let conflict = crate::input_evidence::conflicts(&target)?;
        crate::input_evidence::resolve(&conflict, policy)?;
        self.track(id)?;
        self.flush()?;
        let before = basecraft_io::hash_file(&target).ok();
        let expected = self.save_hashes_at(id, &target)?;
        if self.log.is_some() {
            let target_value = serde_json::to_value(&target)?;
            let intent = serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1, "session": self.id, "target": target_value,
                "before_hash": before, "expected_hashes": expected,
                "status": "pending", "note": "data and provenance are separate writes"
            }))?;
            std::fs::create_dir_all(&self.storage_root)?;
            basecraft_io::write_atomic(&self.storage_root.join("pending-save.json"), |w| {
                w.write_all(&intent)
            })?;
        }
        let outcome = self.save_inner(id, path);
        if outcome.is_ok() {
            if target == self.resolve(&self.docs[id.0].key) {
                self.docs[id.0].saved_state_hashes = self.save_hashes_at(id, &target)?;
                self.docs[id.0].disk_stamp = crate::input_evidence::stamp(&target)?;
            }
            if self.log.is_some() {
                std::fs::remove_file(self.storage_root.join("pending-save.json"))?;
            }
        }
        outcome.map_err(|error| {
            let now = basecraft_io::hash_file(&target).ok();
            if now != before && now.as_ref() == expected.first() {
                // Our main-file replacement completed. Accept precisely that
                // generation for retry, while retaining old sidecar checks.
                if target == self.resolve(&self.docs[id.0].key) {
                    let target_key = self.relative(&target);
                    for (key, hash) in &mut self.docs[id.0].accepted_hashes {
                        if *key == target_key {
                            hash.clone_from(&expected[0]);
                        }
                    }
                    if let Some(expected_sidecar) = expected.get(1) {
                        let sidecar = if self.is_tree(id) || self.is_delimitation(id) {
                            files::tree_sidecar(&target)
                        } else {
                            files::gff_sidecar(&target)
                        };
                        if basecraft_io::hash_file(&sidecar).ok().as_ref() == Some(expected_sidecar)
                        {
                            let key = self.relative(&sidecar);
                            self.docs[id.0].absent_sidecars.retain(|p| *p != key);
                            self.docs[id.0].accepted_hashes.retain(|(p, _)| *p != key);
                            self.docs[id.0]
                                .accepted_hashes
                                .push((key, expected_sidecar.clone()));
                        }
                    }
                }
                invalid(format!(
                    "data saved; provenance or sidecar incomplete: {error}. Recovery remains at {}",
                    self.storage_root.display()
                ))
            } else {
                error
            }
        })
    }

    fn save_inner(&mut self, id: DocId, path: Option<&Path>) -> Result<()> {
        self.flush()?;
        let target = path.map_or_else(|| self.resolve(&self.docs[id.0].key), Path::to_path_buf);
        // An unchanged ordinary save neither rewrites data nor creates history.
        self.track(id)?;
        if target == self.resolve(&self.docs[id.0].key) {
            for key in &self.docs[id.0].absent_sidecars {
                if self.resolve(key).exists() {
                    return Err(invalid(
                        "a source sidecar appeared since opening; reopen before saving",
                    ));
                }
            }
            for (key, hash) in &self.docs[id.0].accepted_hashes {
                crate::save_guard::check_hash(&self.resolve(key), hash)?;
            }
        }
        let format = files::Format::from_path(&target)?;
        let mut hashes = Vec::new();
        let mut main_hash = String::new();
        let opened = match &self.docs[id.0].content {
            Loaded::Delimitation(work) => {
                if format != files::Format::Delimitation {
                    return Err(invalid(
                        "a delimitation workspace requires SPART or SPART-XML",
                    ));
                }
                let new_run = if crate::analysis_store::ArtifactRun::for_result(&target)?.is_none()
                {
                    Some(crate::analysis_store::ArtifactRun::create(
                        &target,
                        &ulid::Ulid::generate().to_string(),
                    )?)
                } else {
                    None
                };
                let xml = files::delimitation_xml(&target);
                basecraft_io::write_atomic(&target, |w| {
                    let mut hw = HashingWriter::new(w);
                    files::write_delimitation(work, xml, &mut hw)?;
                    main_hash = hw.hash();
                    Ok(())
                })?;
                let sidecar = if let Some(run) = &new_run {
                    run.artifact(&format!(
                        "{}.basecraft.json",
                        target
                            .file_name()
                            .and_then(|n| n.to_str())
                            .ok_or_else(|| invalid("SPART filename must be UTF-8"))?
                    ))?
                } else {
                    files::tree_sidecar(&target)
                };
                let mut side_hash = String::new();
                basecraft_io::write_atomic(&sidecar, |w| {
                    let mut hw = HashingWriter::new(w);
                    files::write_delimitation_sidecar_at(work, &main_hash, &target, &mut hw)?;
                    side_hash = hw.hash();
                    Ok(())
                })?;
                if let Some(run) = &new_run {
                    let input = crate::input_evidence::SubmittedInput {
                        schema_version: 1,
                        source: None,
                        submitted_hash: main_hash.clone(),
                        submitted_size: std::fs::metadata(&target)?.len(),
                        transformation: "identity-v1".into(),
                        state: "generated".into(),
                        extra: std::collections::BTreeMap::new(),
                    };
                    basecraft_io::write_atomic(&run.artifact("submitted.fasta")?, |w| {
                        std::io::copy(&mut std::fs::File::open(&target)?, w)?;
                        Ok(())
                    })?;
                    crate::input_evidence::write_json(&run.artifact("input.json")?, &input)?;
                    crate::input_evidence::write_json(
                        &run.artifact("run.json")?,
                        &serde_json::json!({"schema_version":1,"id":run.id(),"result":target.file_name().and_then(|n| n.to_str()),"status":"saved"}),
                    )?;
                    crate::input_evidence::register(run)?;
                    run.bind(&target)?;
                }
                if let Some(run) = crate::analysis_store::ArtifactRun::for_result(&target)? {
                    let mut input = crate::input_evidence::read_input(&run)?;
                    let mut upstream = input
                        .extra
                        .get("upstream")
                        .and_then(serde_json::Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for dependency in crate::delimitation::dependencies(work)? {
                        let dependency = dependency.canonicalize()?;
                        if dependency == run {
                            continue;
                        }
                        let bytes =
                            crate::provenance::read_record(&dependency.join("manifest.json"))?;
                        let publication: basecraft_protocol::provenance::Publication =
                            serde_json::from_slice(&bytes)?;
                        let reference = serde_json::json!({"bundle":crate::input_evidence::relative(&run, &dependency)?,"id":publication.id,"hash":basecraft_io::hash_bytes(&bytes)});
                        if !upstream.contains(&reference) {
                            upstream.push(reference);
                        }
                    }
                    if !upstream.is_empty() {
                        input
                            .extra
                            .insert("upstream".into(), serde_json::json!(upstream));
                    }
                    crate::input_evidence::write_json(&run.join("input.json"), &input)?;
                }
                hashes.push((self.relative(&target), main_hash));
                hashes.push((self.relative(&sidecar), side_hash));
                return self.log_save(id, &target, hashes);
            }
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
            Loaded::Tree(t) => {
                if format != files::Format::Tree {
                    return Err(invalid("a tree can only be saved as a tree file"));
                }
                let original = self.resolve(&self.docs[id.0].key);
                let save_as = crate::links::document_entry(&original)?
                    != crate::links::document_entry(&target)?;
                let (hashes, outputs) = if save_as {
                    let links = crate::links::PreparedLinks::prepare(&original, &target)?;
                    links.publish(|| self.save_tree(t, &target))?
                } else {
                    (self.save_tree(t, &target)?, Vec::new())
                };
                self.log_save(id, &target, hashes)?;
                if !outputs.is_empty() {
                    self.log_tool_run(
                        id,
                        self.actor.clone(),
                        ("basecraft.links", "copy"),
                        serde_json::json!({"from": original,"to":target}),
                        &[],
                        &outputs,
                    )?;
                }
                return Ok(());
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

    /// Write a tree and its sidecar; returns their hashes.
    fn save_tree(&self, tree: &OpenedTree, target: &Path) -> Result<Vec<(String, String)>> {
        let mut hashes = Vec::new();
        let sidecar = files::tree_sidecar(target);
        for (path, sidecar_file) in [(target, false), (sidecar.as_path(), true)] {
            let mut hash = String::new();
            basecraft_io::write_atomic(path, |w| {
                let mut hw = HashingWriter::new(w);
                if sidecar_file {
                    files::write_tree_sidecar(&tree.doc, &mut hw)?;
                } else {
                    files::write_tree(tree, &mut hw)?;
                }
                hash = hw.hash();
                Ok(())
            })?;
            hashes.push((self.relative(path), hash));
        }
        Ok(hashes)
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
        self.write(&entry)?;
        if target == self.resolve(&self.docs[id.0].key) {
            self.docs[id.0]
                .accepted_hashes
                .clone_from(&entry.result.hashes);
            self.docs[id.0].absent_sidecars.clear();
        }
        if self.tracked
            && self.provenance_policy != basecraft_platform::settings::ProvenancePolicy::None
        {
            self.publish_save(id, target, &entry, &[], None)
                .map_err(|e| {
                    invalid(format!(
                        "data saved; provenance incomplete: {e}. Recovery has been retained at {}",
                        self.storage_root.display()
                    ))
                })?;
        }
        Ok(())
    }

    /// Publish exact submitted analysis inputs and its output ancestry after a
    /// validated successful `ToolRun`. Failed attempts remain local job evidence.
    pub fn publish_analysis(
        &self,
        id: DocId,
        target: &Path,
        inputs: &[(String, String)],
    ) -> Result<()> {
        if !self.tracked
            || self.provenance_policy == basecraft_platform::settings::ProvenancePolicy::None
        {
            return Ok(());
        }
        let mut activity = self
            .events
            .iter()
            .rev()
            .find(|e| e.doc == self.docs[id.0].key && matches!(e.op, Op::ToolRun { .. }))
            .cloned()
            .ok_or_else(|| invalid("analysis has no recorded activity"))?;
        activity.result.hashes.retain(|pair| !inputs.contains(pair));
        self.publish_save(id, target, &activity, inputs, None)
    }

    /// Publish against the evidence frozen when the job was submitted.
    pub fn publish_analysis_snapshot(
        &self,
        id: DocId,
        target: &Path,
        inputs: &[(String, String)],
        snapshot: &crate::document_record::DocumentRecord,
    ) -> Result<()> {
        if !self.tracked
            || self.provenance_policy == basecraft_platform::settings::ProvenancePolicy::None
        {
            return Ok(());
        }
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        let mut activity = self
            .events
            .iter()
            .rev()
            .find(|e| e.doc == slot.key && matches!(e.op, Op::ToolRun { .. }))
            .cloned()
            .ok_or_else(|| invalid("analysis has no recorded activity"))?;
        activity.result.hashes.retain(|pair| !inputs.contains(pair));
        self.publish_save(id, target, &activity, inputs, Some(snapshot))
    }

    fn publish_save(
        &self,
        id: DocId,
        target: &Path,
        save: &LogEntry,
        analysis_inputs: &[(String, String)],
        snapshot: Option<&crate::document_record::DocumentRecord>,
    ) -> Result<()> {
        use basecraft_protocol::provenance::{
            ArtifactRevision, Completeness, EditActivity, Publication,
        };
        let bundle = match self.provenance_policy {
            basecraft_platform::settings::ProvenancePolicy::Portable => {
                if let Some(hidden) = crate::analysis_store::ArtifactRun::for_result(target)? {
                    hidden
                } else if self.root.join(".basecraft/project.toml").is_file() {
                    self.root.join(".basecraft/provenance").join(
                        basecraft_io::hash_bytes(self.relative(target).as_bytes())
                            .trim_start_matches("b3:"),
                    )
                } else {
                    crate::provenance::companion(target)
                }
            }
            basecraft_platform::settings::ProvenancePolicy::LocalOnly => {
                self.local_provenance_root.join(
                    basecraft_io::hash_bytes(target.as_os_str().as_encoded_bytes())
                        .trim_start_matches("b3:"),
                )
            }
            basecraft_platform::settings::ProvenancePolicy::None => return Ok(()),
        };
        std::fs::create_dir_all(bundle.join("artifacts"))?;
        let source_bundle = snapshot.and_then(|r| r.origins.first()).map_or_else(
            || self.docs[id.0].origin_bundle.clone(),
            |(path, _)| path.clone(),
        );
        let previous_bundle = if bundle.join("manifest.json").exists() {
            &bundle
        } else {
            &source_bundle
        };
        let previous = if snapshot.is_some() {
            snapshot
                .and_then(|r| r.origins.first())
                .map(|(_, p)| p.clone())
        } else if previous_bundle.join("manifest.json").is_file() {
            let publication: Publication = serde_json::from_slice(
                &crate::provenance::read_record(&previous_bundle.join("manifest.json"))?,
            )?;
            if publication.schema_version != 1 {
                return Err(invalid("unsupported parent provenance version"));
            }
            Some(publication)
        } else {
            None
        };
        let baseline_hash = self
            .events
            .iter()
            .find(|e| e.doc == self.docs[id.0].key && matches!(e.op, Op::Open { .. }))
            .and_then(|e| e.result.hashes.first())
            .map(|(_, hash)| hash);
        let previous = previous.filter(|p| {
            snapshot.is_some()
                || p.session == self.id
                || p.outputs.first().map(|a| &a.hash) == baseline_hash
        });
        let mut parents = previous
            .as_ref()
            .map(|p| vec![p.id.clone()])
            .unwrap_or_default();
        // Save As carries the contributing history rather than leaving dangling
        // references into the source's companion.
        if previous.is_some() && previous_bundle != &bundle {
            for subdirectory in ["artifacts", "records"] {
                let directory = previous_bundle.join(subdirectory);
                std::fs::create_dir_all(bundle.join(subdirectory))?;
                if directory.exists() {
                    for entry in std::fs::read_dir(directory)? {
                        let entry = entry?;
                        if !entry.file_type()?.is_file() {
                            return Err(invalid("parent bundle contains non-file ancestry"));
                        }
                        let destination = bundle.join(subdirectory).join(entry.file_name());
                        if !destination.exists() {
                            basecraft_io::write_atomic(&destination, |w| {
                                std::io::copy(&mut std::fs::File::open(entry.path())?, w)?;
                                Ok(())
                            })?;
                        }
                    }
                }
            }
            if self.is_delimitation(id) {
                // Species publications retain immutable generated files in their
                // run. Copied parent records still resolve those checked paths
                // in the new bundle; clean source references remain references.
                for entry in std::fs::read_dir(previous_bundle.join("records"))? {
                    let entry = entry?;
                    let record: Publication =
                        serde_json::from_slice(&crate::provenance::read_record(&entry.path())?)?;
                    for output in record.outputs {
                        let Some(name) = output.retained else {
                            continue;
                        };
                        if Path::new(&name)
                            .components()
                            .any(|c| !matches!(c, std::path::Component::Normal(_)))
                        {
                            return Err(invalid("Unsafe retained species output path"));
                        }
                        let destination = bundle.join(&name);
                        if destination.is_file() {
                            continue;
                        }
                        let source = previous_bundle.join(&name);
                        if basecraft_io::hash_file(&source)? != output.hash {
                            return Err(invalid("Retained species output changed"));
                        }
                        if let Some(parent) = destination.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        basecraft_io::write_atomic(&destination, |w| {
                            std::io::copy(&mut std::fs::File::open(&source)?, w)?;
                            Ok(())
                        })?;
                        if basecraft_io::hash_file(&destination)? != output.hash {
                            return Err(invalid("Species output changed during Save As"));
                        }
                    }
                }
            }
        }
        // Save As keeps a live reference to the contributing run's evidence
        // decisions; it does not automatically copy unchanged source files.
        if previous.is_some()
            && previous_bundle != &bundle
            && previous_bundle.join("input.json").is_file()
        {
            let mut input = crate::input_evidence::read_input(previous_bundle)?;
            let mut upstream = input
                .extra
                .get("upstream")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for reference in &mut upstream {
                let path = reference["bundle"]
                    .as_str()
                    .ok_or_else(|| invalid("ancestor location missing"))?;
                reference["bundle"] = serde_json::json!(crate::input_evidence::relative(
                    &bundle.canonicalize()?,
                    &previous_bundle.join(path).canonicalize()?
                )?);
            }
            if bundle.join("input.json").is_file() {
                let current = crate::input_evidence::read_input(&bundle)?;
                if let Some(dependencies) = current
                    .extra
                    .get("upstream")
                    .and_then(serde_json::Value::as_array)
                {
                    for dependency in dependencies {
                        if !upstream.contains(dependency) {
                            upstream.push(dependency.clone());
                        }
                    }
                }
            }
            if !upstream.is_empty() {
                input
                    .extra
                    .insert("upstream".into(), serde_json::json!(upstream));
            }
            if input.source.is_some() {
                input.extra.insert(
                    "origin_run".into(),
                    serde_json::json!(crate::input_evidence::relative(
                        &bundle.canonicalize()?,
                        &previous_bundle.canonicalize()?
                    )?),
                );
            } else if previous_bundle.join("submitted.fasta").is_file() {
                basecraft_io::write_atomic(&bundle.join("submitted.fasta"), |w| {
                    std::io::copy(
                        &mut std::fs::File::open(previous_bundle.join("submitted.fasta"))?,
                        w,
                    )?;
                    Ok(())
                })?;
            }
            crate::input_evidence::write_json(&bundle.join("input.json"), &input)?;
        }
        let retain =
            |path: &Path, hash: &str, revision: String, name: String| -> Result<ArtifactRevision> {
                let bytes = std::fs::read(path)?;
                if basecraft_io::hash_bytes(&bytes) != hash {
                    return Err(invalid("artifact changed during provenance publication"));
                }
                let relative = format!("artifacts/{}", hash.trim_start_matches("b3:"));
                let dest = bundle.join(&relative);
                if !dest.exists() {
                    basecraft_io::write_atomic(&dest, |w| w.write_all(&bytes))?;
                } else if basecraft_io::hash_file(&dest)? != hash {
                    return Err(invalid("retained artifact is corrupt"));
                }
                Ok(ArtifactRevision {
                    id: revision,
                    logical_name: name,
                    hash: hash.into(),
                    size: bytes.len() as u64,
                    retained: Some(relative),
                    reference: None,
                })
            };
        let mut inputs = Vec::new();
        for event in self.events.iter().filter(|e| {
            snapshot.is_none_or(|r| r.document == self.docs[id.0].key)
                && e.doc == self.docs[id.0].key
                && matches!(e.op, Op::Open { .. })
        }) {
            for (key, hash) in &event.result.hashes {
                let mut source =
                    objects_dir(&self.storage_root).join(hash.trim_start_matches("b3:"));
                if !self.store_objects {
                    let objects = self
                        .memory_objects
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let bytes = objects
                        .get(hash)
                        .ok_or_else(|| invalid("original input snapshot unavailable"))?;
                    source = bundle
                        .join("artifacts")
                        .join(hash.trim_start_matches("b3:"));
                    basecraft_io::write_atomic(&source, |w| w.write_all(bytes))?;
                }
                inputs.push(retain(
                    &source,
                    hash,
                    format!("{}:input:{}", event.id, inputs.len()),
                    Path::new(key)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                )?);
            }
        }
        for (key, hash) in analysis_inputs {
            let mut source = objects_dir(&self.storage_root).join(hash.trim_start_matches("b3:"));
            if !self.store_objects {
                let objects = self
                    .memory_objects
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let bytes = objects
                    .get(hash)
                    .ok_or_else(|| invalid("submitted input snapshot unavailable"))?;
                source = bundle
                    .join("artifacts")
                    .join(hash.trim_start_matches("b3:"));
                basecraft_io::write_atomic(&source, |w| w.write_all(bytes))?;
            }
            inputs.push(retain(
                &source,
                hash,
                format!("{}:submitted:{}", save.id, inputs.len()),
                Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )?);
        }
        let mut outputs = Vec::new();
        for (key, hash) in &save.result.hashes {
            let logical_name = if crate::analysis_store::ArtifactRun::for_result(target)?.is_some()
            {
                crate::input_evidence::relative(
                    &target.parent().unwrap_or(Path::new(".")).canonicalize()?,
                    &self.resolve(key).canonicalize()?,
                )?
            } else {
                Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            };
            outputs.push(retain(
                &self.resolve(key),
                hash,
                format!("{}:output:{}", save.id, outputs.len()),
                logical_name,
            )?);
        }
        // Public reports use logical names and anonymous agents. Original local
        // journals retain exact paths for recovery; those paths are not portable.
        let mut events: Vec<_> = self
            .events
            .iter()
            .filter(|e| e.doc == self.docs[id.0].key)
            .cloned()
            .collect();
        for event in &mut events {
            event.doc = target
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            event.host.clear();
            if matches!(event.actor, Actor::Human { .. }) {
                event.actor = Actor::Human {
                    user: "redacted".into(),
                };
            }
            match &mut event.op {
                Op::Open { path } | Op::Save { path } => path.clone_from(&event.doc),
                _ => {}
            }
            redact_operation_paths(&mut event.op);
            for (key, _) in &mut event.result.hashes {
                *key = Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            }
        }
        let mut effective_edits: Vec<_> = self.docs[id.0]
            .undo
            .iter()
            .map(|a| EditActivity {
                id: a.entry_id.clone(),
                operation: a.op.clone(),
            })
            .collect();
        if matches!(save.op, Op::ToolRun { .. }) {
            effective_edits.push(EditActivity {
                id: save.id.clone(),
                operation: save.op.clone(),
            });
        }
        for activity in &mut effective_edits {
            redact_operation_paths(&mut activity.operation);
        }
        let inherited = if snapshot.is_some() {
            previous
        } else {
            self.docs[id.0].origin.clone().or(previous)
        };
        if let Some(previous) = inherited {
            if previous.session != self.id {
                let mut inherited = previous.effective_edits;
                inherited.extend(effective_edits);
                effective_edits = inherited;
                let mut inherited_events = previous.events;
                inherited_events.extend(events);
                events = inherited_events;
                for input in previous.inputs {
                    if !inputs.iter().any(|a| a.id == input.id) {
                        inputs.push(input);
                    }
                }
            }
        }
        if let Some(snapshot) = snapshot {
            events.clone_from(&snapshot.events);
            events.push(save.clone());
            effective_edits.clone_from(&snapshot.effective);
            effective_edits.push(EditActivity {
                id: save.id.clone(),
                operation: save.op.clone(),
            });
        }
        for event in &mut events {
            event.host.clear();
            event.doc = Path::new(&event.doc)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if matches!(event.actor, Actor::Human { .. }) {
                event.actor = Actor::Human {
                    user: "redacted".into(),
                };
            }
            redact_operation_paths(&mut event.op);
            for (key, _) in &mut event.result.hashes {
                *key = Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            }
        }
        for activity in &mut effective_edits {
            redact_operation_paths(&mut activity.operation);
        }
        if let Some(snapshot) = snapshot {
            for (origin_bundle, origin) in &snapshot.origins {
                if !parents.contains(&origin.id) {
                    parents.push(origin.id.clone());
                }
                for input in &origin.inputs {
                    if !inputs.iter().any(|a| a.id == input.id) {
                        inputs.push(input.clone());
                    }
                }
                if origin_bundle != &bundle {
                    for folder in ["records", "artifacts"] {
                        std::fs::create_dir_all(bundle.join(folder))?;
                        for entry in std::fs::read_dir(origin_bundle.join(folder))? {
                            let entry = entry?;
                            if !entry.file_type()?.is_file() {
                                return Err(invalid("ancestor contains a non-file record"));
                            }
                            let destination = bundle.join(folder).join(entry.file_name());
                            if !destination.exists() {
                                basecraft_io::write_atomic(&destination, |w| {
                                    std::io::copy(&mut std::fs::File::open(entry.path())?, w)?;
                                    Ok(())
                                })?;
                            }
                        }
                    }
                }
            }
        }
        let mut parent_hashes = std::collections::BTreeMap::new();
        for parent in &parents {
            let path = bundle.join("records").join(format!("{parent}.json"));
            parent_hashes.insert(parent.clone(), basecraft_io::hash_file(&path)?);
        }
        let publication = Publication {
            schema_version: 1,
            id: save.id.clone(),
            session: self.id.clone(),
            parents,
            parent_hashes,
            created: save.t.clone(),
            origin: "external history before import is unknown".into(),
            inputs,
            outputs,
            events,
            effective_edits,
            completeness: Completeness {
                ancestry_known: false,
                input_bytes_available: true,
                execution_specification_available: false,
                replay_verified: false,
            },
            report_template_version: 2,
        };
        crate::provenance::publish(&bundle, &publication)
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
        let _evidence_lock = crate::input_evidence::lock()?;
        crate::input_evidence::resolve(
            &crate::input_evidence::conflicts(path)?,
            crate::input_evidence::SavePolicy::Cancel,
        )?;
        self.track(id)?;
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
        self.track(id)?;
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

    pub fn log_referenced_tool_run(
        &mut self,
        id: DocId,
        actor: Actor,
        (plugin, operation): (&str, &str),
        params: serde_json::Value,
        inputs: &[(String, String)],
        outputs: &[PathBuf],
    ) -> Result<()> {
        self.reference_baseline(id)?;
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

    /// Write a derived file, such as a figure, and log it.
    pub fn export(
        &mut self,
        id: DocId,
        path: &Path,
        format: &str,
        f: impl FnOnce(&mut dyn std::io::Write) -> std::io::Result<()>,
    ) -> Result<()> {
        let _evidence_lock = crate::input_evidence::lock()?;
        crate::input_evidence::resolve(
            &crate::input_evidence::conflicts(path)?,
            crate::input_evidence::SavePolicy::Cancel,
        )?;
        self.track(id)?;
        self.flush()?;
        let mut hash = String::new();
        basecraft_io::write_atomic(path, |w| {
            let mut hw = HashingWriter::new(w);
            f(&mut hw)?;
            hash = hw.hash();
            Ok(())
        })?;
        let key = self.relative(path);
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Export {
                path: key.clone(),
                format: format.to_string(),
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
        let rec = files::extract_record(&self.sequence_of(id)?.doc, start, end, segments, reverse);
        let mut hw = HashingWriter::new(std::io::sink());
        basecraft_io::genbank::write(&mut hw, &rec)?;
        Ok(hw.hash())
    }

    /// Hashes the files a save would write, without writing them.
    pub fn save_hashes(&self, id: DocId) -> Result<Vec<String>> {
        self.save_hashes_at(id, Path::new(&self.docs[id.0].key))
    }

    /// Replay a save using its actual exchange format, including Save As.
    pub fn save_hashes_at(&self, id: DocId, target: &Path) -> Result<Vec<String>> {
        let opened = match &self.docs[id.0].content {
            Loaded::Delimitation(work) => {
                let mut main = HashingWriter::new(std::io::sink());
                files::write_delimitation(work, files::delimitation_xml(target), &mut main)?;
                let hash = main.hash();
                let mut side = HashingWriter::new(std::io::sink());
                files::write_delimitation_sidecar_at(
                    work,
                    &hash,
                    &self.resolve(&target.to_string_lossy()),
                    &mut side,
                )?;
                return Ok(vec![hash, side.hash()]);
            }
            Loaded::Sequence(o) => o,
            Loaded::Alignment(a) => {
                let mut hw = HashingWriter::new(std::io::sink());
                files::write_alignment(&a.doc, &mut hw)?;
                return Ok(vec![hw.hash()]);
            }
            Loaded::Tree(t) => {
                let mut hw = HashingWriter::new(std::io::sink());
                files::write_tree(t, &mut hw)?;
                let mut side = HashingWriter::new(std::io::sink());
                files::write_tree_sidecar(&t.doc, &mut side)?;
                return Ok(vec![hw.hash(), side.hash()]);
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

    /// Retain failure and cancellation diagnostics without creating a hypothesis.
    pub fn log_job_finished(
        &mut self,
        id: DocId,
        report: basecraft_protocol::delimitation::JobRecord,
    ) -> Result<()> {
        self.track(id)?;
        self.flush()?;
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::JobFinished { report },
            OpResult::default(),
        );
        self.write(&entry)
    }

    /// A referenced job outcome does not require promoting the source to an edit journal.
    pub fn log_referenced_job_finished(
        &mut self,
        id: DocId,
        report: basecraft_protocol::delimitation::JobRecord,
    ) -> Result<()> {
        self.reference_baseline(id)?;
        self.flush()?;
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::JobFinished { report },
            OpResult::default(),
        );
        self.write(&entry)
    }

    // Operations.

    /// Apply an edit as the session's default actor.
    pub fn apply(&mut self, doc: DocId, op: Op) -> Result<Outcome> {
        let actor = self.actor.clone();
        self.apply_as(doc, op, actor)
    }

    pub fn apply_as(&mut self, doc: DocId, op: Op, actor: Actor) -> Result<Outcome> {
        // Prepare fallible tree edits before tracking. Rejection must not create
        // recovery records, mutate supports, or clear the redo stack.
        let prepared_tree = if matches!(
            op,
            Op::TreeMidpointRoot {} | Op::TreeMinimumVarianceRoot {} | Op::TreeStyle { .. }
        ) {
            let slot = self
                .docs
                .get(doc.0)
                .ok_or_else(|| invalid("unknown document"))?;
            let Loaded::Tree(tree) = &slot.content else {
                return Err(invalid(format!("{} needs a tree", op.name())));
            };
            let mut candidate = tree.doc.clone();
            execute_tree(&mut candidate, &op)?;
            Some(candidate)
        } else {
            None
        };
        self.track(doc)?;
        if self.merge && self.try_merge(doc, &op, &actor)? {
            self.docs[doc.0].generation += 1;
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
        let (inverse, feature) = if let Some(candidate) = prepared_tree {
            let Loaded::Tree(tree) = &mut self.docs[doc.0].content else {
                return Err(invalid("tree edit needs a tree"));
            };
            (
                Inverse::Tree(Box::new(std::mem::replace(&mut tree.doc, candidate))),
                None,
            )
        } else {
            self.execute(doc, &op)?
        };
        self.docs[doc.0].generation += 1;
        let result = OpResult {
            hashes: Vec::new(),
            feature: feature.map(|f| f.0),
        };
        let entry = self.entry(doc, actor, op.clone(), result);
        let entry_id = entry.id.clone();
        let mergeable = mergeable(&op);
        if !(self.merge && mergeable) {
            if let Err(error) = self.write(&entry) {
                restore_inverse(&mut self.docs[doc.0].content, &inverse)?;
                self.docs[doc.0].generation -= 1;
                return Err(error);
            }
        }
        let slot = &mut self.docs[doc.0];
        slot.redo.clear();
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
            Loaded::Delimitation(work) => {
                let Op::DelimitationEdit { edit } = op else {
                    return Err(invalid("this needs a delimitation edit"));
                };
                let before = Box::new(work.clone());
                basecraft_docs::delimitation::apply(work, edit).map_err(|e| invalid(e.0))?;
                return Ok((Inverse::Delimitation(before), None));
            }
            Loaded::Alignment(a) => {
                return execute_alignment(a, op).map(|u| (Inverse::Alignment(u), None));
            }
            Loaded::Tree(t) => {
                return execute_tree(&mut t.doc, op).map(|before| (Inverse::Tree(before), None));
            }
        };
        execute_sequence(doc, op)
    }

    /// Validate/stage the full proposal batch before logging or live mutation.
    /// Ordinary I/O failure rolls the journal back; power loss uses recovery.
    pub fn apply_sequence_proposals(
        &mut self,
        id: DocId,
        generation: u64,
        revision: u64,
        ops: &[Op],
        actor: &Actor,
    ) -> Result<()> {
        let slot = self
            .docs
            .get(id.0)
            .ok_or_else(|| invalid("unknown document"))?;
        let Loaded::Sequence(sequence) = &slot.content else {
            return Err(invalid("proposals require a sequence document"));
        };
        if slot.generation != generation || sequence.doc.version() != revision {
            return Err(invalid("stale_revision: proposal target changed"));
        }
        if ops.len() > 4096 {
            return Err(invalid("proposal count exceeds 4096"));
        }
        let mut candidate = sequence.doc.clone();
        let mut prepared = Vec::new();
        let mut entries = Vec::new();
        for op in ops {
            if !matches!(
                op,
                Op::SeqReplace { .. }
                    | Op::FeatureAdd { .. }
                    | Op::FeatureRemove { .. }
                    | Op::FeatureUpdate { .. }
            ) {
                return Err(invalid(
                    "proposal contains a prohibited host/lifecycle operation",
                ));
            }
            let (inverse, feature) = execute_sequence(&mut candidate, op)?;
            let mut entry = self.entry(
                id,
                actor.clone(),
                op.clone(),
                OpResult {
                    hashes: vec![],
                    feature: feature.map(|f| f.0),
                },
            );
            entry.state = DocState {
                len: candidate.len(),
                features: candidate.features().count() as u64,
                rows: None,
            };
            prepared.push((op.clone(), inverse, entry.id.clone()));
            entries.push(entry);
        }
        if ops.is_empty() {
            return Ok(());
        }
        self.flush()?;
        self.track(id)?;
        if let Some(log) = &mut self.log {
            log.append_batch(&entries)?;
        }
        if self.tracked {
            self.events.extend(entries);
        }
        let group = self.next_group;
        self.next_group += 1;
        let slot = &mut self.docs[id.0];
        let Loaded::Sequence(sequence) = &mut slot.content else {
            return Err(invalid("proposal document kind changed"));
        };
        sequence.doc = candidate;
        slot.generation += ops.len() as u64;
        slot.redo.clear();
        slot.undo
            .extend(prepared.into_iter().map(|(op, inverse, entry_id)| Applied {
                entry_id,
                op,
                inverse,
                group: Some(group),
            }));
        Ok(())
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
            (Loaded::Tree(t), Inverse::Tree(before)) => t.doc = (**before).clone(),
            (Loaded::Delimitation(work), Inverse::Delimitation(before)) => {
                *work = (**before).clone();
            }
            (Loaded::Sequence(o), inverse) => undo_sequence(&mut o.doc, inverse),
            (Loaded::Alignment(_) | Loaded::Tree(_) | Loaded::Delimitation(_), _) => {
                return Err(invalid("undo of the wrong kind of edit"));
            }
        }
        self.docs[id.0].generation += 1;
        let of = applied.entry_id.clone();
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Undo { of: of.clone() },
            OpResult::default(),
        );
        if let Err(error) = self.write(&entry) {
            let inverse = self.reexecute(id, &applied)?;
            self.docs[id.0].generation -= 1;
            self.docs[id.0].undo.push(Applied { inverse, ..applied });
            return Err(error);
        }
        self.docs[id.0].redo.push(applied);
        Ok(of)
    }

    fn redo_one(&mut self, id: DocId) -> Result<String> {
        self.flush()?;
        let applied = self.docs[id.0].redo.pop().ok_or(Error::NothingTo("redo"))?;
        let inverse = self.reexecute(id, &applied)?;
        self.docs[id.0].generation += 1;
        let of = applied.entry_id.clone();
        let entry = self.entry(
            id,
            self.actor.clone(),
            Op::Redo { of: of.clone() },
            OpResult::default(),
        );
        if let Err(error) = self.write(&entry) {
            restore_inverse(&mut self.docs[id.0].content, &inverse)?;
            self.docs[id.0].generation -= 1;
            self.docs[id.0].redo.push(applied);
            return Err(error);
        }
        self.docs[id.0].undo.push(Applied { inverse, ..applied });
        Ok(of)
    }
    fn reexecute(&mut self, id: DocId, applied: &Applied) -> Result<Inverse> {
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
        Ok(inverse)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let flushed = self.flush().is_ok();
        if flushed
            && self.storage_root != self.root
            && !self.storage_root.join("pending-save.json").exists()
            && self.docs.iter().enumerate().all(|(i, slot)| {
                self.save_hashes(DocId(i)).ok().as_ref() == Some(&slot.saved_state_hashes)
            })
        {
            let _ = std::fs::remove_dir_all(&self.storage_root);
        }
    }
}

/// Draw a tree as another view keeps it: replay its drawing edits in order.
/// An edit that no longer applies, such as rotating a clade the tree lacks,
/// is skipped.
pub fn apply_drawing_edits(
    doc: &mut TreeDoc,
    edits: &[basecraft_protocol::delimitation::TreeDrawingEdit],
) {
    for edit in edits {
        let _ = execute_tree(doc, &edit.to_op());
    }
}

/// Carry out a tree edit; returns the tree as it was.
fn execute_sequence(doc: &mut SequenceDoc, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
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
        | Op::AlnFill { .. }
        | Op::AlnInsertGaps { .. }
        | Op::AlnDelete { .. }
        | Op::AlnShift { .. }
        | Op::AlnRemoveGapColumns {}
        | Op::AlnMoveRows { .. }
        | Op::AlnRenameRow { .. }
        | Op::AlnAddSequences { .. }
        | Op::AlnRemoveRows { .. } => Err(invalid(format!("{} needs an alignment", op.name()))),
        Op::TreeReroot { .. }
        | Op::TreeMidpointRoot {}
        | Op::TreeMinimumVarianceRoot {}
        | Op::TreeRootPosition { .. }
        | Op::TreeRotate { .. }
        | Op::TreeLadderize { .. }
        | Op::TreeRenameTip { .. }
        | Op::TreeAnnotate { .. }
        | Op::TreeAddSupport { .. }
        | Op::TreeRemoveSupport { .. }
        | Op::TreeStyle { .. } => Err(invalid(format!("{} needs a tree", op.name()))),
        Op::DelimitationEdit { .. } => Err(invalid("this needs a delimitation workspace")),
        Op::JobFinished { .. } => Err(invalid("use log_job_finished")),
        Op::Open { .. }
        | Op::Save { .. }
        | Op::Extract { .. }
        | Op::Export { .. }
        | Op::ToolRun { .. }
        | Op::Undo { .. }
        | Op::Redo { .. } => Err(invalid(format!("{} is not an edit", op.name()))),
    }
}

fn execute_tree(doc: &mut TreeDoc, op: &Op) -> Result<Box<TreeDoc>> {
    let before = Box::new(doc.clone());
    let edit = match op {
        Op::TreeReroot { clade } => doc.reroot(clade),
        Op::TreeMidpointRoot {} => doc.midpoint_root(),
        Op::TreeMinimumVarianceRoot {} => doc.minimum_variance_root(),
        Op::TreeRootPosition { clade, fraction } => doc.set_root_position(clade, *fraction),
        Op::TreeRotate { clade } => doc.rotate(clade),
        Op::TreeLadderize { small_first } => {
            doc.ladderize(*small_first);
            Ok(())
        }
        Op::TreeRenameTip { from, to } => doc.rename_tip(from, to),
        Op::TreeAnnotate { id, annotation } => doc.annotate(*id, annotation.clone()),
        Op::TreeAddSupport {
            name,
            source,
            values,
            kind,
        } => doc
            .add_support(name, source, values)
            .map(|()| doc.set_support_kind(name, *kind)),
        Op::TreeRemoveSupport { name } => doc.remove_support(name),
        Op::TreeStyle {
            style,
            remove_supports,
        } => {
            if style
                .support_decimals
                .values()
                .any(|&decimals| decimals > 10)
            {
                return Err(invalid("support decimal places must be between 0 and 10"));
            }
            let mut candidate = doc.clone();
            candidate.set_style(style.clone());
            remove_supports
                .iter()
                .try_for_each(|name| candidate.remove_support(name))
                .map(|()| *doc = candidate)
        }
        _ => {
            return Err(invalid(format!(
                "{} needs a sequence or an alignment",
                op.name()
            )));
        }
    };
    edit.map_err(|e| invalid(e.0))?;
    Ok(before)
}

fn redact_operation_paths(operation: &mut Op) {
    fn scrub(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) if Path::new(text).is_absolute() => {
                *text = Path::new(text)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    scrub(value);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    scrub(value);
                }
            }
            _ => {}
        }
    }
    if let Op::ToolRun {
        inputs,
        outputs,
        params,
        ..
    } = operation
    {
        for name in inputs.iter_mut().chain(outputs.iter_mut()) {
            *name = Path::new(name)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
        }
        scrub(params);
    }
}

fn restore_inverse(content: &mut Loaded, inverse: &Inverse) -> Result<()> {
    match (content, inverse) {
        (Loaded::Sequence(opened), inverse) => undo_sequence(&mut opened.doc, inverse),
        (Loaded::Alignment(opened), Inverse::Alignment(steps)) => opened.doc.undo(steps),
        (Loaded::Tree(opened), Inverse::Tree(before)) => opened.doc = (**before).clone(),
        (Loaded::Delimitation(work), Inverse::Delimitation(before)) => *work = (**before).clone(),
        _ => return Err(invalid("edit rollback has incompatible document type")),
    }
    Ok(())
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
        Inverse::Alignment(_) | Inverse::Tree(_) | Inverse::Delimitation(_) => {}
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
        Op::AlnInsert {
            rows,
            col,
            residues,
        } => {
            let residues = normalise(residues)?;
            doc.insert(rows, *col, residues.as_bytes())
        }
        Op::AlnFill {
            rows,
            start,
            end,
            residue,
        } => {
            let residue = normalise(residue)?;
            let &[b] = residue.as_bytes() else {
                return Err(invalid(format!(
                    "aln.fill takes one residue, not {residue:?}"
                )));
            };
            doc.fill(rows, *start, *end, b)
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
        Op::AlnAddSequences { rows, reference } => {
            let rows = rows
                .iter()
                .map(|(name, seq)| Ok((name.clone(), normalise(seq)?)))
                .collect::<Result<Vec<_>>>()?;
            doc.add_sequences(&rows, reference.as_deref())
        }
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
            Op::AlnInsert {
                rows,
                col,
                residues,
            },
            Op::AlnInsert {
                rows: rows2,
                col: col2,
                residues: residues2,
            },
        ) if rows == rows2 && *col2 == col + residues.len() as u64 => Some(Op::AlnInsert {
            rows: rows.clone(),
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
        return Err(invalid(format!("{c:?} is not a base or amino acid")));
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

impl Session {
    /// Reference-first publication: actual submitted input identity and the
    /// transformation are recorded in input.json; clean source bytes are not copied.
    pub fn publish_referenced_analysis(
        &self,
        id: DocId,
        target: &Path,
        outputs: &[PathBuf],
        snapshot: &crate::document_record::DocumentRecord,
        run: &crate::analysis_store::ArtifactRun,
    ) -> Result<()> {
        use basecraft_protocol::provenance::{
            ArtifactRevision, Completeness, EditActivity, Publication,
        };
        let mut input = crate::input_evidence::read_input(run.directory())?;
        let mut upstream = input
            .extra
            .get("upstream")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (bundle, origin) in &snapshot.origins {
            let mut bytes = serde_json::to_vec_pretty(origin)?;
            bytes.push(b'\n');
            upstream.push(serde_json::json!({"bundle":crate::input_evidence::relative(run.directory(), &bundle.canonicalize()?)?,"id":origin.id,"hash":basecraft_io::hash_bytes(&bytes)}));
        }
        if !upstream.is_empty() {
            input
                .extra
                .insert("upstream".into(), serde_json::json!(upstream));
            crate::input_evidence::write_json(&run.directory().join("input.json"), &input)?;
        }
        let activity = self
            .events
            .iter()
            .rev()
            .find(|e| e.doc == self.docs[id.0].key && matches!(e.op, Op::ToolRun { .. }))
            .ok_or_else(|| invalid("analysis has no recorded activity"))?
            .clone();
        std::fs::create_dir_all(run.directory().join("artifacts"))?;
        let source = if let Some(reference) = input.source.clone() {
            ArtifactRevision {
                id: format!("{}:source", run.id()),
                logical_name: Path::new(&reference.path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hash: reference.hash.clone(),
                size: reference.stamp.size,
                retained: None,
                reference: Some(reference),
            }
        } else {
            ArtifactRevision {
                id: format!("{}:submitted", run.id()),
                logical_name: "submitted.fasta".into(),
                hash: input.submitted_hash.clone(),
                size: input.submitted_size,
                retained: Some("submitted.fasta".into()),
                reference: None,
            }
        };
        let mut inputs = vec![source];
        if let Some(retained) = input.extra.get("retained_inputs") {
            let additional: Vec<ArtifactRevision> = serde_json::from_value(retained.clone())?;
            if additional.len() > 64 {
                return Err(invalid("too many retained analysis inputs"));
            }
            for artifact in additional {
                let name = artifact
                    .retained
                    .as_deref()
                    .ok_or_else(|| invalid("auxiliary input is not retained"))?;
                if name.is_empty()
                    || Path::new(name)
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                    || artifact.reference.is_some()
                {
                    return Err(invalid("unsafe retained analysis input"));
                }
                let path = run.directory().join(name);
                let metadata = std::fs::symlink_metadata(&path)?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || path.canonicalize()? != path
                    || metadata.len() != artifact.size
                    || basecraft_io::hash_file(&path)? != artifact.hash
                {
                    return Err(invalid("retained analysis input changed"));
                }
                inputs.push(artifact);
            }
        }
        let mut artifacts = Vec::new();
        for path in outputs {
            let retained = if !input.extra.contains_key("species_result") {
                None
            } else if *path == files::tree_sidecar(target) {
                let hash = basecraft_io::hash_file(path)?;
                let name = format!("artifacts/{}", hash.trim_start_matches("b3:"));
                basecraft_io::write_atomic(&run.directory().join(&name), |w| {
                    std::io::copy(&mut std::fs::File::open(path)?, w)?;
                    Ok(())
                })?;
                Some(name)
            } else {
                path.strip_prefix(run.directory())
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(str::to_owned)
            };
            artifacts.push(ArtifactRevision {
                id: format!("{}:output:{}", run.id(), artifacts.len()),
                logical_name: crate::input_evidence::relative(
                    target
                        .parent()
                        .unwrap_or(Path::new("."))
                        .canonicalize()?
                        .as_path(),
                    path,
                )?,
                hash: basecraft_io::hash_file(path)?,
                size: std::fs::metadata(path)?.len(),
                retained,
                reference: None,
            });
        }
        let mut events = snapshot.events.clone();
        events.push(activity.clone());
        let mut effective = snapshot.effective.clone();
        effective.push(EditActivity {
            id: activity.id.clone(),
            operation: activity.op,
        });
        for event in &mut events {
            event.host.clear();
            event.doc = Path::new(&event.doc)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if matches!(event.actor, Actor::Human { .. }) {
                event.actor = Actor::Human {
                    user: "redacted".into(),
                };
            }
            redact_operation_paths(&mut event.op);
            for (key, _) in &mut event.result.hashes {
                *key = Path::new(key)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            }
        }
        for edit in &mut effective {
            redact_operation_paths(&mut edit.operation);
        }
        let publication = Publication {
            schema_version: 1, id: activity.id, session: self.id.clone(), parents: Vec::new(), parent_hashes: std::collections::BTreeMap::new(),
            created: activity.t, origin: "Referenced source; submitted transformation recorded in input.json. Declared immutable upstream records are referenced without copying input bytes. Earlier history before import remains unknown.".into(),
            inputs, outputs: artifacts, events, effective_edits: effective,
            completeness: Completeness { ancestry_known: false, input_bytes_available: input.state != "broken", execution_specification_available: false, replay_verified: false }, report_template_version: 2,
        };
        match self.provenance_policy {
            basecraft_platform::settings::ProvenancePolicy::Portable => {
                crate::provenance::publish(run.directory(), &publication)?;
            }
            basecraft_platform::settings::ProvenancePolicy::LocalOnly => {
                let bundle = self.local_provenance_root.join(
                    basecraft_io::hash_bytes(target.as_os_str().as_encoded_bytes())
                        .trim_start_matches("b3:"),
                );
                std::fs::create_dir_all(bundle.join("artifacts"))?;
                let mut local_input = input.clone();
                local_input.extra.insert(
                    "origin_run".into(),
                    serde_json::json!(crate::input_evidence::relative(&bundle, run.directory())?),
                );
                crate::input_evidence::write_json(&bundle.join("input.json"), &local_input)?;
                if input.source.is_none() {
                    basecraft_io::write_atomic(&bundle.join("submitted.fasta"), |w| {
                        std::io::copy(
                            &mut std::fs::File::open(run.directory().join("submitted.fasta"))?,
                            w,
                        )?;
                        Ok(())
                    })?;
                }
                for artifact in &publication.inputs {
                    if let Some(name) = artifact
                        .retained
                        .as_deref()
                        .filter(|n| *n != "submitted.fasta")
                    {
                        basecraft_io::write_atomic(&bundle.join(name), |w| {
                            std::io::copy(
                                &mut std::fs::File::open(run.directory().join(name))?,
                                w,
                            )?;
                            Ok(())
                        })?;
                    }
                }
                crate::provenance::publish(&bundle, &publication)?;
            }
            basecraft_platform::settings::ProvenancePolicy::None => {}
        }
        if input.extra.contains_key("species_result")
            && !run.directory().join("manifest.json").is_file()
        {
            std::fs::create_dir_all(run.directory().join("records"))?;
            crate::input_evidence::write_json(
                &run.directory()
                    .join("records")
                    .join(format!("{}.json", publication.id)),
                &publication,
            )?;
            crate::input_evidence::write_json(
                &run.directory().join("manifest.json"),
                &publication,
            )?;
        }
        crate::input_evidence::write_json(
            &run.directory().join("run.json"),
            &serde_json::json!({"schema_version":1,"id":run.id(),"result":target.file_name().and_then(|n| n.to_str()).ok_or_else(|| invalid("result filename is not UTF-8"))?,"status":"completed","outputs":publication.outputs}),
        )?;
        run.bind(target)
    }
}
