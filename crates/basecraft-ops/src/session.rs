//! A session: open documents, the operations applied to them, undo and redo,
//! and the log.
//!
//! Every change goes through [`Session::apply`], [`Session::undo`] or
//! [`Session::redo`], so the log is complete. Every edit is journaled at
//! once. Consecutive single-place insertions by the same actor within a
//! second share one undo step, like an editor's undo grouping. Alignment
//! edits group the same way: typing over residues along a row, gaps inserted
//! one after another, a block slid step by step. A cursor jump, another
//! command, another document, undo or redo ends the burst.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use basecraft_docs::alignment::Undo as AlnUndo;
use basecraft_docs::{AlignmentDoc, EditResult, FeatureId, SequenceDoc, TreeDoc};
use basecraft_protocol::ops::LOG_VERSION;
use basecraft_protocol::{Actor, DocState, Feature, LogEntry, Op, OpResult};

use crate::documents::execute_tree;
use crate::error::invalid;
use crate::files::{self, Loaded, Opened};
use crate::log::LogWriter;

mod alphabet;
mod creation;
mod extraction;
mod history;
mod open;
use open::SavedMark;
mod proposals;
mod retained;
use retained::Retained;
#[cfg(test)]
mod retention_tests;
mod save;
pub(crate) mod save_consent;
mod save_decision;
#[cfg(test)]
mod save_failure_tests;
#[cfg(test)]
mod saved_mark_tests;
mod tool_runs;
pub use open::{objects_dir, recovery_folder};
pub use proposals::{ProposalResult, SequenceOffer};
pub use save_decision::{EvidenceAction, SaveCheck, SaveDecision, SaveOutcome, SaveWarning};
pub use tool_runs::ToolRunRecord;
pub mod support_import;
use crate::{Error, Result};

/// Typing within this interval, continuing the previous insertion, joins its
/// undo step. Each keystroke is still journaled on its own.
const TYPING_WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocId(pub usize);

pub(crate) use history::{Inverse, MetadataUndo, SubmissionUndo, WorkspaceUndo};

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
    generation: u64,
    pending_check: save_consent::PendingSaveCheck,
    undo: Vec<Applied>,
    redo: Vec<Applied>,
    /// Edits older than the undo limit: still in effect, no longer undoable.
    settled: Retained<basecraft_protocol::provenance::EditActivity>,
    baseline: Option<LogEntry>,
    baseline_logged: bool,
    accepted_hashes: Vec<(String, String)>,
    saved_at: Option<SavedMark>,
    recovery_boundary: Option<String>,
    source_stamps: Vec<(PathBuf, Option<basecraft_protocol::provenance::FileStamp>)>,
    /// What the editor should say about this document's recovery.
    recovery_notice: Option<String>,
    /// The previous Save As target this session wrote, so a partial Save As can retry it.
    saved_target: Option<(PathBuf, basecraft_protocol::provenance::FileStamp, String)>,
    absent_sidecars: Vec<String>,
    origin: Option<basecraft_protocol::provenance::Publication>,
    origin_issue: Option<String>,
    origin_bundle: PathBuf,
    disk_stamp: basecraft_protocol::provenance::FileStamp,
    /// The next save may drop fields a later basecraft wrote (D14).
    drop_newer_fields: bool,
}

/// Undo reaches back at least this many steps; older edits are settled.
const UNDO_STEPS: usize = 10_000;
/// Journal events kept in memory; older ones move to a temporary file.
const EVENTS_IN_MEMORY: usize = 4_096;

impl Slot {
    /// Settle the oldest undo steps beyond the limit, whole groups at a time.
    /// At least one step stays, so the saved mark still tells an edited
    /// document from a clean one. Returns the groups of the settled steps.
    fn settle(&mut self) -> Vec<u64> {
        if self.undo.len() <= UNDO_STEPS + UNDO_STEPS / 4 {
            return Vec::new();
        }
        let mut count = self.undo.len() - UNDO_STEPS;
        while count + 1 < self.undo.len()
            && self.undo[count].group.is_some()
            && self.undo[count].group == self.undo[count - 1].group
        {
            count += 1;
        }
        self.settle_oldest(count)
    }

    /// Settle the oldest `count` undo steps; returns their groups.
    fn settle_oldest(&mut self, count: usize) -> Vec<u64> {
        let mut groups = Vec::new();
        self.settled.extend(self.undo.drain(..count).map(|applied| {
            groups.extend(applied.group);
            basecraft_protocol::provenance::EditActivity {
                id: applied.entry_id,
                operation: applied.op,
            }
        }));
        groups
    }

    /// The undo stack, or the redo stack.
    fn stack(&mut self, undo: bool) -> &mut Vec<Applied> {
        if undo { &mut self.undo } else { &mut self.redo }
    }

    /// Every edit in effect, oldest first.
    fn effective(&self) -> Result<Vec<basecraft_protocol::provenance::EditActivity>> {
        let mut effective = self.settled.matching(|_| true)?;
        effective.extend(self.undo.iter().map(|applied| {
            basecraft_protocol::provenance::EditActivity {
                id: applied.entry_id.clone(),
                operation: applied.op.clone(),
            }
        }));
        Ok(effective)
    }
}

/// The typing burst the next keystroke may join: its document, actor, last
/// edit and undo group.
struct Typing {
    doc: DocId,
    actor: Actor,
    op: Op,
    group: u64,
    last: Instant,
}

/// What `apply` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub entry_id: String,
    pub feature: Option<FeatureId>,
    /// Things the user should hear about, such as a label a reroot moved.
    pub notes: Vec<String>,
    /// Labels this edit displaced; the window asks where each goes (D11).
    pub displaced: Vec<basecraft_protocol::tree::DisplacedLabel>,
}

/// Target-save and evidence locks held while writing an export or extract.
struct DerivedTarget {
    _evidence: crate::records::input_evidence::SaveLock,
}

enum ReplayMode {
    Live,
    Historical,
}

pub struct Session {
    root: PathBuf,
    /// The folder the user chose, or none. Set once, when the session is made.
    project: Option<crate::project::Project>,
    storage_root: PathBuf,
    /// Where Sanger review sources go when their own folder is read-only.
    local_provenance_root: PathBuf,
    tracked: bool,
    write_publication: crate::documents::WritePublication,
    evidence_actions: Mutex<Vec<save_decision::EvidenceAction>>,
    replay_mode: ReplayMode,
    events: Retained<LogEntry>,
    pending_baselines: Vec<LogEntry>,
    id: String,
    host: String,
    actor: Actor,
    log: Option<LogWriter>,
    docs: Vec<Slot>,
    typing: Option<Typing>,
    store_objects: bool,
    /// The open undo group, if any, and the next group number.
    group: Option<u64>,
    next_group: u64,
    /// The documents each undo group changed, so a group spanning several
    /// documents is undone and redone in all of them or refused.
    group_docs: std::collections::BTreeMap<u64, std::collections::BTreeSet<usize>>,
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
            local_provenance_root: root.join(".basecraft/provenance"),
            tracked: true,
            write_publication: crate::documents::WritePublication::Immediate,
            evidence_actions: Mutex::new(Vec::new()),
            replay_mode: ReplayMode::Live,
            events: Retained::new(EVENTS_IN_MEMORY),
            pending_baselines: Vec::new(),
            project: None,
            root,
            id: ulid::Ulid::generate().to_string(),
            host,
            actor: Actor::Human {
                user: basecraft_platform::username(),
            },
            log,
            docs: Vec::new(),
            typing: None,
            store_objects: true,
            group: None,
            next_group: 1,
            group_docs: std::collections::BTreeMap::new(),
            ids: Mutex::new(ulid::Generator::new()),
        }
    }

    /// A viewing session whose first edit starts recovery in
    /// `<root>/.basecraft/recovery/<session>`: the project's folder, or the
    /// folder of a loose file. Nothing is created before the first edit.
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
        session.storage_root = recovery_folder(&session.root).join(&session.id);
        session.local_provenance_root = paths.state.join("provenance");
        session.log = preferences
            .files
            .recovery
            .then(|| LogWriter::new(&session.storage_root, &session.id));
        session.store_objects = preferences.files.recovery;
        Ok(session)
    }

    pub fn recovery_root(&self) -> &Path {
        &self.storage_root
    }

    /// A session that logs nothing, for replay and tests.
    pub fn unlogged(root: impl Into<PathBuf>) -> Self {
        let mut s = Self::detached(root);
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
        s
    }

    /// Give the session the project the user chose. Opening files never
    /// changes it; without one, every file is a loose file.
    #[must_use]
    pub fn in_project(mut self, project: Option<crate::project::Project>) -> Self {
        self.project = project;
        self
    }

    pub fn project(&self) -> Option<&crate::project::Project> {
        self.project.as_ref()
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

    /// The Sanger review payload for a checked handle.
    pub fn sanger_review(&self, id: DocId) -> Result<&crate::sanger_review::SangerReview> {
        self.content(id)?.sanger_review().ok_or(Error::WrongKind {
            id,
            expected: files::Kind::SangerReview,
        })
    }

    /// A sequence document. Returns an error for a missing ID or a different document kind.
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

    pub fn is_tree(&self, id: DocId) -> bool {
        self.docs
            .get(id.0)
            .is_some_and(|slot| slot.content.kind() == files::Kind::Tree)
    }

    /// Why a document's companion or metadata was set aside when it opened.
    pub fn opening_warning(&self, id: DocId) -> Result<Option<&str>> {
        Ok(self.content(id)?.as_document().opening_warning())
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

    fn slot(&self, id: DocId) -> Result<&Slot> {
        self.docs.get(id.0).ok_or(Error::UnknownDocument { id })
    }

    pub(crate) fn content(&self, id: DocId) -> Result<&Loaded> {
        Ok(&self.slot(id)?.content)
    }

    /// Every record, with the current unsaved first record replacing its disk view.
    pub fn blast_records(&self, id: DocId) -> Result<Vec<crate::blast::Query>> {
        self.checked_sequence(id)?;
        let opened = self.sequence_of(id)?;
        let mut queries = if let Some(fasta) = &opened.fasta {
            crate::blast::from_fasta(fasta.bytes(), self.doc_key(id)?)?
        } else {
            Vec::new()
        };
        if let Some(molecule) = crate::blast::stated_molecule(&opened.doc.alphabet()) {
            for query in &mut queries {
                query.source["molecule"] = molecule.as_str().into();
            }
        }
        let mut current = crate::blast::from_doc(&opened.doc, None, None)?;
        current.source["session"] = serde_json::json!(self.id());
        current.source["generation"] = serde_json::json!(self.generation(id)?);
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

    pub fn doc_key(&self, id: DocId) -> Result<&str> {
        Ok(&self.slot(id)?.key)
    }

    /// Whether `id` names a document in this session. Check before using an ID
    /// that came from outside the process.
    pub fn is_open(&self, id: DocId) -> bool {
        id.0 < self.docs.len()
    }

    pub fn generation(&self, id: DocId) -> Result<u64> {
        Ok(self.slot(id)?.generation)
    }

    /// The edit revision callers pin a request to: the document version, or the
    /// generation for workspace, metadata and submission documents without a separate version.
    pub fn revision(&self, id: DocId) -> Result<u64> {
        let slot = self.slot(id)?;
        Ok(slot.content.as_document().revision(slot.generation))
    }

    pub fn can_undo(&self, id: DocId) -> Result<bool> {
        Ok(!self.slot(id)?.undo.is_empty())
    }

    pub fn can_redo(&self, id: DocId) -> Result<bool> {
        Ok(!self.slot(id)?.redo.is_empty())
    }

    fn relative(&self, path: &Path) -> String {
        relative(&self.root, path)
    }

    /// What provenance publication reads about one document.
    fn history(&self, id: DocId) -> Result<crate::records::publish::History<'_>> {
        let slot = self.slot(id)?;
        Ok(crate::records::publish::History {
            session: &self.id,
            tracked: self.tracked,
            root: &self.root,
            project: self.project.as_ref(),
            storage_root: &self.storage_root,
            events: self.events.matching(|event| event.doc == slot.key)?,
            pending: &self.pending_baselines,
            key: &slot.key,
            effective: slot.effective()?,
            origin: slot.origin.as_ref(),
            origin_bundle: &slot.origin_bundle,
            delimitation: slot.content.kind() == files::Kind::Delimitation,
        })
    }

    /// Publish an analysis against the evidence frozen when its job was submitted.
    pub fn publish_analysis_snapshot(
        &self,
        id: DocId,
        target: &Path,
        inputs: &[(String, String)],
        snapshot: &crate::records::DocumentRecord,
    ) -> Result<()> {
        self.history(id)?.analysis(target, inputs, snapshot)
    }

    /// Add a captured input revision and its producing activity to an
    /// operation's existing inventory.
    pub fn complete_operation_publication(
        &self,
        id: DocId,
        snapshot: &crate::records::DocumentRecord,
        run: &crate::records::ArtifactRun,
    ) -> Result<()> {
        self.history(id)?.complete_operation(snapshot, run)
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
    pub fn state(&self, id: DocId) -> Result<DocState> {
        Ok(self.slot(id)?.content.as_document().fingerprint())
    }
}

impl Drop for Session {
    /// Remove recovery once nothing is unsaved, then any emptied recovery and
    /// `.basecraft` folder.
    fn drop(&mut self) {
        let unsaved = self.storage_root.join("pending-save.json").exists()
            || !self.docs.iter().all(Slot::is_clean);
        if self.storage_root != self.root && !unsaved {
            let _ = std::fs::remove_dir_all(&self.storage_root);
            let _ = std::fs::remove_dir(recovery_folder(&self.root));
            let _ = std::fs::remove_dir(self.root.join(".basecraft"));
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

fn restore_inverse(content: &mut Loaded, inverse: &Inverse) -> Result<()> {
    content.as_document_mut().revert(inverse)
}

/// Edits that can join a typing burst.
fn typing(op: &Op) -> bool {
    match op {
        Op::SeqReplace {
            start,
            end,
            residues,
            ..
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

/// Project-relative path, or the absolute path for files outside it.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    abs.strip_prefix(&root).map_or_else(
        |_| abs.to_string_lossy().into_owned(),
        |p| p.to_string_lossy().into_owned(),
    )
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
