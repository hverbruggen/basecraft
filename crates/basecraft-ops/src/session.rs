//! A session: open documents, the operations applied to them, undo and redo,
//! and the log.
//!
//! Every change goes through [`Session::apply`], [`Session::undo`] or
//! [`Session::redo`], so the log is complete. Consecutive single-place
//! insertions by the same actor within a second merge into one operation,
//! like an editor's undo grouping, so a burst of typing is one log entry.
//! Alignment edits merge the same way: typing over residues along a row,
//! gaps inserted one after another, a block slid step by step.

pub(crate) mod save_attempt_observation;
use save_attempt_observation::{
    Disposition as SaveDisposition, Hooks as SaveHooks, Legacy as LegacySave,
    Milestone as SaveMilestone, Site as SaveSite,
};

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use basecraft_docs::alignment::Undo as AlnUndo;
use basecraft_docs::{AlignmentDoc, EditResult, FeatureId, SequenceDoc, TreeDoc};
use basecraft_protocol::ops::LOG_VERSION;
use basecraft_protocol::{Actor, DocState, Feature, LogEntry, Op, OpResult};

use crate::documents::execute_tree;
use crate::error::invalid;
use crate::files::{self, Loaded, Opened};
use crate::log::LogWriter;

mod creation;
mod extraction;
mod history;
mod open;
mod proposals;
mod publish;
#[cfg(test)]
mod retention_tests;
mod save;
pub(crate) mod save_authority;
mod save_decision;
#[cfg(test)]
mod saved_fingerprint_tests;
mod tool_runs;
pub use save_decision::{EvidenceAction, SaveCheck, SaveDecision, SaveOutcome, SaveWarning};
pub mod support_import;
use crate::{Error, Result};

/// Typing within this interval, at the end of the previous insertion,
/// merges into one operation.
const MERGE_WINDOW: Duration = Duration::from_secs(1);

/// Files up to this size are copied into the project's object store when
/// opened, so replay works after the original is overwritten.
pub const OBJECT_STORE_LIMIT: u64 = 100 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocId(pub usize);

pub(crate) use history::Inverse;

pub(crate) struct Applied {
    pub(crate) entry_id: String,
    pub(crate) op: Op,
    pub(crate) inverse: Inverse,
    /// Operations applied together (a plugin's results) share a group and
    /// undo and redo together.
    pub(crate) group: Option<u64>,
}

struct Slot {
    key: String,
    content: Loaded,
    current_saved_hashes: OnceLock<Vec<String>>,
    generation: u64,
    save_authority: save_authority::PendingSaveAuthority,
    undo: Vec<Applied>,
    redo: Vec<Applied>,
    baseline: Option<LogEntry>,
    baseline_logged: bool,
    accepted_hashes: Vec<(String, String)>,
    saved_state_hashes: Vec<String>,
    saved_target: Option<(PathBuf, basecraft_protocol::provenance::FileStamp, String)>,
    absent_sidecars: Vec<String>,
    origin: Option<basecraft_protocol::provenance::Publication>,
    origin_issue: Option<String>,
    origin_bundle: PathBuf,
    disk_stamp: basecraft_protocol::provenance::FileStamp,
}

impl Slot {
    /// Invalidate before every live mutable borrow, including error rollback.
    fn content_mut(&mut self) -> &mut Loaded {
        self.current_saved_hashes.take();
        &mut self.content
    }
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

/// Target-save and evidence locks held while writing an export or extract.
struct DerivedTarget {
    _target: Option<std::fs::File>,
    _evidence: std::fs::File,
}

enum ReplayMode {
    Live,
    Historical,
}

pub struct Session {
    root: PathBuf,
    storage_root: PathBuf,
    provenance_policy: basecraft_platform::settings::ProvenancePolicy,
    local_provenance_root: PathBuf,
    tracked: bool,
    write_publication: crate::documents::WritePublication,
    evidence_actions: Mutex<Vec<save_decision::EvidenceAction>>,
    replay_mode: ReplayMode,
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
            write_publication: crate::documents::WritePublication::Immediate,
            evidence_actions: Mutex::new(Vec::new()),
            replay_mode: ReplayMode::Live,
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

    /// Historical replay uses recorded plan results, without consulting today's source files.
    pub(crate) fn replay_session(root: &Path) -> Self {
        let mut session = Self::unlogged(root);
        session.replay_mode = ReplayMode::Historical;
        session
    }

    /// A session that keeps its activity in memory and writes no journal.
    /// Host workflow stages record and publish into their own analysis runs
    /// through it; the owning session records the workflow run itself.
    pub fn detached(root: impl Into<PathBuf>) -> Self {
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

    /// A sequence document. Returns an error for a missing ID or a different document kind.
    pub fn sanger_review(&self, id: DocId) -> Result<&crate::sanger_review::SangerReview> {
        self.content(id)?.sanger_review().ok_or(Error::WrongKind {
            id,
            expected: files::Kind::SangerReview,
        })
    }

    pub fn doc(&self, id: DocId) -> Result<&SequenceDoc> {
        Ok(&self.sequence_of(id)?.doc)
    }

    /// An alignment document. Returns an error for a missing ID or a different document kind.
    pub fn alignment(&self, id: DocId) -> Result<&AlignmentDoc> {
        self.content(id)?
            .alignment()
            .ok_or(Error::WrongKind {
                id,
                expected: files::Kind::Alignment,
            })
            .map(|payload| &payload.doc)
    }

    /// A tree document. Returns an error for a missing ID or a different document kind.
    pub fn tree(&self, id: DocId) -> Result<&TreeDoc> {
        self.content(id)?
            .tree()
            .ok_or(Error::WrongKind {
                id,
                expected: files::Kind::Tree,
            })
            .map(|payload| &payload.doc)
    }

    pub fn delimitation(
        &self,
        id: DocId,
    ) -> Result<&basecraft_protocol::delimitation::DelimitationWorkspace> {
        self.content(id)?.delimitation().ok_or(Error::WrongKind {
            id,
            expected: files::Kind::Delimitation,
        })
    }

    pub fn is_delimitation(&self, id: DocId) -> bool {
        self.docs
            .get(id.0)
            .is_some_and(|slot| slot.content.kind() == files::Kind::Delimitation)
    }

    /// The project's specimen metadata. Returns an error for a missing ID or a different document kind.
    pub fn metadata(&self, id: DocId) -> Result<&basecraft_protocol::metadata::ProjectMetadata> {
        self.content(id)?
            .metadata()
            .ok_or(Error::WrongKind {
                id,
                expected: files::Kind::Metadata,
            })
            .map(|payload| &payload.metadata)
    }

    /// What was noticed when the metadata was opened, such as tables edited
    /// outside Basecraft.
    pub fn metadata_notices(&self, id: DocId) -> Result<&[String]> {
        self.content(id)?
            .metadata()
            .ok_or(Error::WrongKind {
                id,
                expected: files::Kind::Metadata,
            })
            .map(|payload| payload.notices.as_slice())
    }

    /// A GenBank submission document. Returns an error for a missing ID or a different document kind.
    pub fn submission(&self, id: DocId) -> Result<&basecraft_protocol::submission::Submission> {
        self.content(id)?.submission().ok_or(Error::WrongKind {
            id,
            expected: files::Kind::Submission,
        })
    }

    pub fn is_submission(&self, id: DocId) -> bool {
        self.docs
            .get(id.0)
            .is_some_and(|slot| slot.content.kind() == files::Kind::Submission)
    }

    pub fn is_metadata(&self, id: DocId) -> bool {
        self.docs
            .get(id.0)
            .is_some_and(|slot| slot.content.kind() == files::Kind::Metadata)
    }

    pub(crate) fn restore_replay_paths(&mut self, id: DocId, from: &Path, to: &Path) {
        self.docs[id.0]
            .content_mut()
            .as_document_mut()
            .rebase_replay_paths(from, to);
    }

    pub(crate) fn legacy_delimitation_hashes(
        &self,
        id: DocId,
        target: &Path,
    ) -> Result<Option<Vec<String>>> {
        self.docs[id.0].content.as_document().legacy_hashes(target)
    }

    pub fn is_tree(&self, id: DocId) -> bool {
        self.docs
            .get(id.0)
            .is_some_and(|slot| slot.content.kind() == files::Kind::Tree)
    }

    pub fn tree_metadata_warning(&self, id: DocId) -> Result<Option<&str>> {
        self.content(id)?
            .tree()
            .ok_or(Error::WrongKind {
                id,
                expected: files::Kind::Tree,
            })
            .map(|payload| payload.metadata_warning.as_deref())
    }

    pub fn is_alignment(&self, id: DocId) -> bool {
        self.docs
            .get(id.0)
            .is_some_and(|slot| slot.content.kind() == files::Kind::Alignment)
    }

    fn sequence_of(&self, id: DocId) -> Result<&Opened> {
        self.content(id)?.sequence().ok_or(Error::WrongKind {
            id,
            expected: files::Kind::Sequence,
        })
    }

    /// Checked access for plugin snapshots; never index a wrong-kind/closed slot.
    pub fn checked_sequence(&self, id: DocId) -> Result<&SequenceDoc> {
        self.doc(id)
    }

    fn content(&self, id: DocId) -> Result<&Loaded> {
        self.docs
            .get(id.0)
            .map(|slot| &slot.content)
            .ok_or(Error::UnknownDocument { id })
    }

    /// Every record, with the current unsaved first record replacing its disk view.
    pub fn blast_records(&self, id: DocId) -> Result<Vec<crate::blast::Query>> {
        self.checked_sequence(id)?;
        let opened = self.sequence_of(id)?;
        let mut queries = if let Some(fasta) = &opened.fasta {
            crate::blast::from_fasta(fasta.bytes(), self.doc_key(id))?
        } else {
            Vec::new()
        };
        let mut current = crate::blast::from_doc(&opened.doc, None, None)?;
        current.source["session"] = serde_json::json!(self.id());
        current.source["generation"] = serde_json::json!(self.generation(id));
        if queries.is_empty() {
            queries.push(current);
        } else {
            queries[0] = current;
        }
        Ok(queries)
    }

    /// The existing project-metadata owner, if open; never create a second owner for a preset.
    pub(crate) fn project_metadata_id(&self) -> Option<DocId> {
        let key = Path::new(crate::metadata::DIRECTORY).join(crate::metadata::MANIFEST);
        self.docs
            .iter()
            .position(|slot| {
                slot.content.kind() == files::Kind::Metadata && Path::new(&slot.key) == key
            })
            .map(DocId)
    }

    pub fn doc_key(&self, id: DocId) -> &str {
        &self.docs[id.0].key
    }

    /// Whether `id` names a document in this session. Check before using an ID
    /// that came from outside the process.
    pub fn is_open(&self, id: DocId) -> bool {
        id.0 < self.docs.len()
    }

    pub fn generation(&self, id: DocId) -> u64 {
        self.docs[id.0].generation
    }

    /// The edit revision callers pin a request to: the document version, or the
    /// generation for workspace, metadata and submission documents without a separate version.
    pub fn revision(&self, id: DocId) -> u64 {
        let slot = &self.docs[id.0];
        slot.content.as_document().revision(slot.generation)
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
        self.docs[id.0].content.as_document().fingerprint()
    }

    // Open and save.

    // Operations.
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

pub(crate) fn portable_analysis_activity(
    snapshot: &crate::document_record::DocumentRecord,
    activity: &LogEntry,
) -> (
    Vec<LogEntry>,
    Vec<basecraft_protocol::provenance::EditActivity>,
) {
    use basecraft_protocol::provenance::EditActivity;
    let mut events = snapshot.events.clone();
    if !events.iter().any(|event| event.id == activity.id) {
        events.push(activity.clone());
    }
    let mut effective = snapshot.effective.clone();
    effective.push(EditActivity {
        id: activity.id.clone(),
        operation: activity.op.clone(),
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
    (events, effective)
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
    content.as_document_mut().revert(inverse)
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

/// Copy a parent bundle's ancestry folder into a new bundle, keeping existing
/// files. A registered run's retained collection lives under `artifacts/` and
/// may hold folders; anything other than files and folders is refused.
fn copy_ancestry(from: &Path, to: &Path, depth: usize) -> Result<()> {
    if depth > 16 {
        return Err(invalid("parent bundle ancestry is nested too deeply"));
    }
    std::fs::create_dir_all(to)?;
    if !from.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let destination = to.join(entry.file_name());
        if kind.is_dir() {
            copy_ancestry(&entry.path(), &destination, depth + 1)?;
        } else if !kind.is_file() {
            return Err(invalid("parent bundle contains non-file ancestry"));
        } else if !destination.exists() {
            basecraft_io::write_atomic(&destination, |w| {
                std::io::copy(&mut std::fs::File::open(entry.path())?, w)?;
                Ok(())
            })?;
        }
    }
    Ok(())
}

pub fn objects_dir(root: &Path) -> PathBuf {
    root.join(".basecraft").join("objects")
}

/// Upper-case residues; reject anything that isn't a letter, `-` or `*`.
pub(crate) fn normalise(residues: &str) -> Result<String> {
    if let Some(c) = residues
        .chars()
        .find(|c| !(c.is_ascii_alphabetic() || *c == '-' || *c == '*'))
    {
        return Err(invalid(format!("{c:?} is not a base or amino acid")));
    }
    Ok(residues.to_ascii_uppercase())
}
