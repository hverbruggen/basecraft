//! Transient desktop-open lifetimes. Session remains the sole document owner.
use basecraft_ops::{
    DocId, Session,
    files::{Format, Kind},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Acquire `path` as a job; `done` gets the candidate on the window thread of
/// `owner`. A failed start comes back as the error.
pub(crate) fn start<T>(
    owner: impl crate::jobs::Owner<T>,
    path: String,
    project: Option<basecraft_ops::project::Project>,
    done: impl FnOnce(std::pin::Pin<&mut T>, Result<Candidate, OpenError>) + Send + 'static,
) -> Result<(), OpenError> {
    crate::jobs::start(
        owner,
        move |work| Ok(Candidate::acquire_in(&path, project, work.flag())),
        |owner, result| done(owner, result.unwrap_or(Err(OpenError::Interrupted))),
    )
    .map(|_| ())
    .map_err(OpenError::Spawn)
}

/// [`start`] in the window's chosen project, as every open is.
pub(crate) fn start_in_chosen<T>(
    owner: impl crate::jobs::Owner<T>,
    path: String,
    done: impl FnOnce(std::pin::Pin<&mut T>, Result<Candidate, OpenError>) + Send + 'static,
) -> Result<(), OpenError> {
    let project = crate::chosen_project::chosen().map_err(basecraft_ops::Error::Invalid)?;
    start(owner, path, project, done)
}

/// A view opening its own file, or reopening after Save As.
pub(crate) fn open_file<T: cxx_qt::Threading + 'static>(
    mut controller: std::pin::Pin<&mut T>,
    path: &cxx_qt_lib::QString,
    completed: impl Fn(std::pin::Pin<&mut T>, Result<Candidate, OpenError>) + Copy + Send + 'static,
) -> bool {
    let thread = controller.qt_thread();
    if let Err(error) = start_in_chosen(thread, path.to_string(), completed) {
        completed(controller.as_mut(), Err(error));
        return false;
    }
    true
}

#[derive(Debug)]
pub(crate) enum OpenError {
    Admission(basecraft_ops::Error),
    Cancelled,
    Interrupted,
    Spawn(std::io::Error),
    Queue,
    RouteMismatch,
    StaleIdentity,
    Exhausted,
}
impl From<basecraft_ops::Error> for OpenError {
    fn from(error: basecraft_ops::Error) -> Self {
        Self::Admission(error)
    }
}
impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // A folder that cannot hold recovery says so with its code (D13).
            Self::Admission(error) => f.write_str(&edit_failure(error)),
            Self::Cancelled => f.write_str("Opening cancelled"),
            Self::Queue => f.write_str("Could not deliver the opened document to the interface"),
            Self::Spawn(error) => write!(f, "Could not start document opening: {error}"),
            Self::Interrupted => f.write_str("Opening interrupted by an internal failure"),
            Self::RouteMismatch => f.write_str("Opened document does not match the canvas route"),
            Self::StaleIdentity => {
                f.write_str("Opened document identity changed before installation")
            }
            Self::Exhausted => f.write_str("No further document requests can be allocated"),
        }
    }
}

pub(crate) fn eligible(path: &str) -> bool {
    matches!(
        Format::from_path(Path::new(path)),
        Ok(Format::Fasta | Format::GenBank | Format::Tree)
    )
}

pub(crate) struct DisplayTree {
    pub path: PathBuf,
    pub tree: basecraft_docs::tree::TreeDoc,
    pub time: Option<basecraft_delimitation::time_tree::TimeTree>,
    pub hash: String,
}

fn display_tree(session: &Session, doc: DocId) -> Result<DisplayTree, String> {
    let work = session.delimitation(doc).map_err(|e| e.to_string())?;
    let path = basecraft_ops::delimitation::checked_display_tree(work)
        .map_err(|e| e.to_string())?
        .ok_or("No display tree")?;
    let (source, hash) = basecraft_io::Source::open_hashed(&path).map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(source.bytes()).map_err(|e| e.to_string())?;
    let trees = basecraft_io::newick::parse_trees(text).map_err(|e| e.to_string())?;
    if trees.len() != 1 {
        return Err("Choose a file containing exactly one display tree".into());
    }
    let tree = basecraft_docs::tree::TreeDoc::from_parsed(&trees[0].name, &trees[0]);
    let time = basecraft_delimitation::time_tree::validate(&tree, true, 1e-4, false).ok();
    Ok(DisplayTree {
        path,
        tree,
        time,
        hash,
    })
}

pub(crate) struct Candidate {
    pub session: Session,
    pub doc: DocId,
    pub kind: Kind,
    pub path: PathBuf,
    pub title_file: String,
    pub root: PathBuf,
    pub display_tree: Option<Result<DisplayTree, String>>,
    generation: u64,
    revision: u64,
    #[cfg(all(test, feature = "test-counters"))]
    pub reads: basecraft_io::counters::Counters,
}
impl Candidate {
    /// Open in the window's chosen project.
    pub fn acquire(path: &str, cancelled: &AtomicBool) -> Result<Self, OpenError> {
        let project = crate::chosen_project::chosen().map_err(basecraft_ops::Error::Invalid)?;
        Self::acquire_in(path, project, cancelled)
    }
    fn acquire_in(
        path: &str,
        project: Option<basecraft_ops::project::Project>,
        cancelled: &AtomicBool,
    ) -> Result<Self, OpenError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(OpenError::Cancelled);
        }
        #[cfg(all(test, feature = "test-counters"))]
        basecraft_io::counters::reset();
        let path = PathBuf::from(path);
        // The project is the folder the user chose; opening never changes it.
        let root = project.as_ref().map_or_else(
            || crate::bridge::file_folder(&path),
            |project| project.root().to_path_buf(),
        );
        let mut session = Session::recoverable(&root)?.in_project(project);
        let (doc, kind) = session.open_classified(&path)?;
        // Offer recovery a crashed session left beside the file or in the
        // project. Only a notice depends on it, so a failed look is no error.
        let _ =
            session.find_earlier_recovery(doc, &[crate::bridge::file_folder(&path), root.clone()]);
        if cancelled.load(Ordering::Acquire) {
            return Err(OpenError::Cancelled);
        }
        Self::prepare(session, doc, kind, path, root)
    }
    pub fn prepare(
        session: Session,
        doc: DocId,
        kind: Kind,
        path: PathBuf,
        root: PathBuf,
    ) -> Result<Self, OpenError> {
        validate(&session, doc, kind)?;
        let display_tree = if kind == Kind::Delimitation
            && session.delimitation(doc)?.view.display_tree.is_some()
        {
            Some(display_tree(&session, doc))
        } else {
            None
        };
        let generation = session.generation(doc)?;
        let revision = session.revision(doc)?;
        let title_file = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let path = path.canonicalize().unwrap_or(path);
        let root = root.canonicalize().unwrap_or(root);
        Ok(Self {
            session,
            doc,
            kind,
            path,
            title_file,
            root,
            display_tree,
            generation,
            revision,
            #[cfg(all(test, feature = "test-counters"))]
            reads: basecraft_io::counters::snapshot(),
        })
    }
    pub fn check(&self, expected: Kind) -> Result<(), OpenError> {
        if self.kind != expected {
            return Err(OpenError::RouteMismatch);
        }
        validate(&self.session, self.doc, expected)?;
        if self.session.generation(self.doc)? != self.generation
            || self.session.revision(self.doc)? != self.revision
        {
            return Err(OpenError::StaleIdentity);
        }
        Ok(())
    }
}
fn validate(session: &Session, doc: DocId, kind: Kind) -> Result<(), OpenError> {
    if !session.is_open(doc) {
        return Err(basecraft_ops::Error::UnknownDocument { id: doc }.into());
    }
    match kind {
        Kind::Sequence => {
            session.doc(doc)?;
        }
        Kind::Alignment => {
            session.alignment(doc)?;
        }
        Kind::Tree => {
            session.tree(doc)?;
        }
        Kind::Delimitation => {
            session.delimitation(doc)?;
        }
        Kind::SangerReview => {
            session.sanger_review(doc)?;
        }
        Kind::Metadata => {
            session.metadata(doc)?;
        }
        Kind::Submission => {
            session.submission(doc)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Queued,
    Loading,
    Acquired,
    Constructing,
    Ready,
}
pub(crate) struct Request {
    pub path: String,
    pub controller: usize,
    pub installed_path: String,
    pub focus: u64,
    pub review: bool,
    pub cancelled: Arc<AtomicBool>,
    pub phase: Phase,
    pub candidate: Option<Candidate>,
}
impl Drop for Request {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
#[derive(Default)]
pub(crate) struct Opens {
    next: u64,
    focus: u64,
    stopped: bool,
    pub requests: BTreeMap<u64, Request>,
}
impl Opens {
    pub fn revoke_focus(&mut self) {
        for request in self.requests.values_mut() {
            request.review = false;
        }
        if let Some(next) = self.focus.checked_add(1) {
            self.focus = next;
        } else {
            for request in self.requests.values_mut() {
                request.focus = 0;
            }
        }
    }
    pub fn begin(&mut self, path: String, review: bool) -> Result<(u64, bool), OpenError> {
        if self.stopped {
            return Err(OpenError::Cancelled);
        }
        for request in self.requests.values_mut() {
            if request.focus != self.focus || request.path != path {
                request.review = false;
            }
        }
        self.focus = self.focus.checked_add(1).ok_or(OpenError::Exhausted)?;
        if let Some((&token, request)) = self
            .requests
            .iter_mut()
            .find(|(_, request)| request.path == path)
        {
            request.focus = self.focus;
            request.review |= review;
            return Ok((token, false));
        }
        self.next = self.next.checked_add(1).ok_or(OpenError::Exhausted)?;
        self.requests.insert(
            self.next,
            Request {
                path,
                controller: 0,
                installed_path: String::new(),
                focus: self.focus,
                review,
                cancelled: Arc::new(AtomicBool::new(false)),
                phase: Phase::Queued,
                candidate: None,
            },
        );
        Ok((self.next, true))
    }
    pub fn track(&mut self, path: String) -> Result<u64, OpenError> {
        if self.stopped {
            return Err(OpenError::Cancelled);
        }
        self.next = self.next.checked_add(1).ok_or(OpenError::Exhausted)?;
        self.requests.insert(
            self.next,
            Request {
                path,
                controller: 0,
                installed_path: String::new(),
                focus: 0,
                review: false,
                cancelled: Arc::new(AtomicBool::new(false)),
                phase: Phase::Ready,
                candidate: None,
            },
        );
        Ok(self.next)
    }
    pub fn retire(&mut self, token: u64) {
        self.requests.remove(&token);
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.requests.clear();
    }
    pub fn wants_focus(&self, token: u64) -> bool {
        !self.stopped
            && self
                .requests
                .get(&token)
                .is_some_and(|r| r.focus == self.focus && !r.cancelled.load(Ordering::Acquire))
    }
}
impl Drop for Opens {
    fn drop(&mut self) {
        self.stop();
    }
}

/// An edit refused because the folder cannot hold recovery (D13) starts with
/// its code, so the window can offer to save a copy and edit that (lane gui2).
pub(crate) fn edit_failure(error: &basecraft_ops::Error) -> String {
    match error {
        basecraft_ops::Error::ReadOnlyFolder { .. } => format!("{}: {error}", error.code()),
        _ => error.to_string(),
    }
}

// All six editors already have a message area; no new presentation or dialog.
pub(crate) fn recovery_notice(
    session: Option<&Session>,
    doc: Option<DocId>,
) -> Option<cxx_qt_lib::QString> {
    session?
        .recovery_warning(doc?)
        .map(cxx_qt_lib::QString::from)
}

/// The edit a folder that cannot hold `.basecraft` refused (D13), for the
/// window to offer a copy: `{"file", "folder", "op"}`. None for other errors.
pub(crate) fn refused_edit(
    session: &Session,
    doc: DocId,
    op: &basecraft_protocol::Op,
    error: &basecraft_ops::Error,
) -> Option<cxx_qt_lib::QString> {
    let basecraft_ops::Error::ReadOnlyFolder { folder, .. } = error else {
        return None;
    };
    let file = session.root().join(session.doc_key(doc).ok()?);
    let text = serde_json::json!({"file": file, "folder": folder, "op": op}).to_string();
    Some(cxx_qt_lib::QString::from(&text))
}

/// Edits waiting for the copy they were moved to, by the copy's path.
static PENDING: std::sync::Mutex<Vec<(PathBuf, basecraft_protocol::Op)>> =
    std::sync::Mutex::new(Vec::new());

/// Copy the file a refused edit was made to, with its companion, to `target`
/// in a folder the user chose, and keep the edit, if any (a refused trace
/// review has none), for when the copy opens.
pub(crate) fn copy_for_editing(refused: &str, target: &Path) -> Result<(), String> {
    let refused: serde_json::Value = serde_json::from_str(refused).map_err(|e| e.to_string())?;
    let source = PathBuf::from(refused["file"].as_str().ok_or("No refused edit")?);
    let op: Option<basecraft_protocol::Op> =
        serde_json::from_value(refused["op"].clone()).map_err(|e| e.to_string())?;
    if target.exists() {
        return Err(format!(
            "{} already exists; choose a new name",
            target.display()
        ));
    }
    std::fs::copy(&source, target).map_err(|e| format!("Could not copy the file: {e}"))?;
    let companion = |file: &Path| {
        let mut name = file.file_name().unwrap_or_default().to_os_string();
        name.push(".basecraft.json");
        file.with_file_name(name)
    };
    if companion(&source).is_file() {
        std::fs::copy(companion(&source), companion(target))
            .map_err(|e| format!("Could not copy the file's companion: {e}"))?;
    }
    if let Some(op) = op {
        let key = target.canonicalize().map_err(|e| e.to_string())?;
        PENDING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((key, op));
    }
    Ok(())
}

/// The refused edit to redo on `path` now that its copy is open.
pub(crate) fn take_refused(path: &Path) -> Option<basecraft_protocol::Op> {
    let key = path.canonicalize().ok()?;
    let mut pending = PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let index = pending.iter().position(|(file, _)| *file == key)?;
    Some(pending.remove(index).1)
}

/// Apply `op` with `apply` (`Session::apply`, `renames::apply_op`, ...), as
/// every editor does: a failure comes back worded by [`edit_failure`], and a
/// refusal for a folder that cannot hold recovery also as the copy offer.
pub(crate) fn applying<T>(
    session: &mut Session,
    doc: DocId,
    op: basecraft_protocol::Op,
    apply: impl FnOnce(&mut Session, DocId, basecraft_protocol::Op) -> basecraft_ops::Result<T>,
) -> (Result<T, String>, Option<cxx_qt_lib::QString>) {
    let kept = op.clone();
    match apply(session, doc, op) {
        Ok(value) => (Ok(value), None),
        Err(error) => (
            Err(edit_failure(&error)),
            refused_edit(session, doc, &kept, &error),
        ),
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn duplicate_paths_order_focus_and_retirement() {
        let mut opens = Opens::default();
        let (a, new) = opens.begin("A.fa".into(), false).unwrap();
        assert!(new);
        let cancel_a = opens.requests[&a].cancelled.clone();
        let (b, _) = opens.begin("B.gb".into(), false).unwrap();
        assert!(!opens.wants_focus(a));
        assert!(opens.wants_focus(b));
        assert_eq!(opens.begin("A.fa".into(), true).unwrap(), (a, false));
        assert!(opens.requests[&a].review);
        assert_eq!(
            opens.requests.keys().copied().collect::<Vec<_>>(),
            vec![a, b]
        );
        opens.revoke_focus();
        assert!(!opens.wants_focus(a));
        opens.retire(a);
        assert!(cancel_a.load(Ordering::Acquire));
        let (reopened, _) = opens.begin("A.fa".into(), false).unwrap();
        assert!(reopened > b);
        assert!(!opens.requests.contains_key(&a));
        let cancel_reopened = opens.requests[&reopened].cancelled.clone();
        opens.stop();
        assert!(cancel_reopened.load(Ordering::Acquire));
        assert!(opens.requests.is_empty());
        assert!(matches!(
            opens.begin("A.fa".into(), false),
            Err(OpenError::Cancelled)
        ));
    }

    #[test]
    fn aliases_are_distinct_and_tokens_never_wrap() {
        let mut opens = Opens::default();
        let (a, _) = opens.begin("A.fa".into(), false).unwrap();
        let (alias, _) = opens.begin("./A.fa".into(), false).unwrap();
        assert_ne!(a, alias);
        opens.next = u64::MAX;
        assert!(matches!(
            opens.begin("C.fa".into(), false),
            Err(OpenError::Exhausted)
        ));
        assert_eq!(opens.requests.len(), 2);
    }

    #[test]
    fn nonreading_gate_preserves_legacy_routes() {
        for path in ["one.fa", "one.GB", "one.nwk", "one.nex"] {
            assert!(eligible(path));
        }
        for path in [
            "one.json",
            "one.submission.json",
            "one.ab1",
            "one.abi",
            "one.spart",
            "one.xml",
            "one.unknown",
        ] {
            assert!(!eligible(path));
        }
    }

    #[test]
    fn identity_and_kind_are_checked_before_indexing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("sequence.gb");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sequences-annotations/locus.gb"),
            &path,
        )
        .unwrap();
        let mut session = Session::unlogged(temp.path());
        let (doc, kind) = session.open_classified(&path).unwrap();
        let mut candidate =
            Candidate::prepare(session, doc, kind, path, temp.path().into()).unwrap();
        assert!(candidate.check(Kind::Sequence).is_ok());
        assert!(matches!(
            candidate.check(Kind::Tree),
            Err(OpenError::RouteMismatch)
        ));
        candidate.generation += 1;
        assert!(matches!(
            candidate.check(Kind::Sequence),
            Err(OpenError::StaleIdentity)
        ));
        candidate.doc = DocId(usize::MAX);
        assert!(matches!(
            candidate.check(Kind::Sequence),
            Err(OpenError::Admission(
                basecraft_ops::Error::UnknownDocument { .. }
            ))
        ));
    }
}
