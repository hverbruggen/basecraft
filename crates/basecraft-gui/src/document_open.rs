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

/// All editor entry points use the same acquisition worker and queued completion.
pub(crate) fn start(
    path: String,
    root: Option<PathBuf>,
    completed: impl FnOnce(Result<Candidate, OpenError>) + Send + 'static,
) -> Result<(), OpenError> {
    crate::document_jobs::background_initialized(
        "document-open",
        move || Candidate::acquire_in(&path, root, &AtomicBool::new(false)),
        || OpenError::Interrupted,
        completed,
    )
    .map_err(OpenError::Spawn)
}

pub(crate) fn open_file<T: cxx_qt::Threading + 'static>(
    mut controller: std::pin::Pin<&mut T>,
    path: &cxx_qt_lib::QString,
    completed: impl Fn(std::pin::Pin<&mut T>, Result<Candidate, OpenError>) + Copy + Send + 'static,
) -> bool {
    let thread = controller.qt_thread();
    let started = start(path.to_string(), None, move |result| {
        let _ = thread.queue(move |controller| completed(controller, result));
    });
    if let Err(error) = started {
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
            Self::Admission(error) => error.fmt(f),
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
    pub fn acquire(path: &str, cancelled: &AtomicBool) -> Result<Self, OpenError> {
        Self::acquire_in(path, None, cancelled)
    }
    fn acquire_in(
        path: &str,
        root: Option<PathBuf>,
        cancelled: &AtomicBool,
    ) -> Result<Self, OpenError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(OpenError::Cancelled);
        }
        #[cfg(all(test, feature = "test-counters"))]
        basecraft_io::counters::reset();
        let path = PathBuf::from(path);
        let root = root.unwrap_or_else(|| crate::bridge::project_root(&path));
        let mut session = Session::recoverable(&root)?;
        let (doc, kind) = session.open_classified(&path)?;
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
        let generation = session.generation(doc);
        let revision = session.revision(doc);
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
        if self.session.generation(self.doc) != self.generation
            || self.session.revision(self.doc) != self.revision
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
    #[allow(dead_code)] // Captured context is never written back on completion.
    pub context: String,
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
    pub fn begin(
        &mut self,
        path: String,
        context: String,
        review: bool,
    ) -> Result<(u64, bool), OpenError> {
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
                context,
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
                context: String::new(),
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

// All six editors already have a message area; no new presentation or dialog.
pub(crate) fn recovery_notice(
    session: Option<&Session>,
    doc: Option<DocId>,
) -> Option<cxx_qt_lib::QString> {
    session?
        .recovery_warning(doc?)
        .map(cxx_qt_lib::QString::from)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn duplicate_paths_order_focus_and_retirement() {
        let mut opens = Opens::default();
        let (a, new) = opens
            .begin("A.fa".into(), "project A".into(), false)
            .unwrap();
        assert!(new);
        let cancel_a = opens.requests[&a].cancelled.clone();
        let (b, _) = opens
            .begin("B.gb".into(), "project B".into(), false)
            .unwrap();
        assert!(!opens.wants_focus(a));
        assert!(opens.wants_focus(b));
        assert_eq!(
            opens
                .begin("A.fa".into(), "different project".into(), true)
                .unwrap(),
            (a, false)
        );
        assert_eq!(opens.requests[&a].context, "project A");
        assert!(opens.requests[&a].review);
        assert_eq!(
            opens.requests.keys().copied().collect::<Vec<_>>(),
            vec![a, b]
        );
        opens.revoke_focus();
        assert!(!opens.wants_focus(a));
        opens.retire(a);
        assert!(cancel_a.load(Ordering::Acquire));
        let (reopened, _) = opens
            .begin("A.fa".into(), "project A".into(), false)
            .unwrap();
        assert!(reopened > b);
        assert!(!opens.requests.contains_key(&a));
        let cancel_reopened = opens.requests[&reopened].cancelled.clone();
        opens.stop();
        assert!(cancel_reopened.load(Ordering::Acquire));
        assert!(opens.requests.is_empty());
        assert!(matches!(
            opens.begin("A.fa".into(), String::new(), false),
            Err(OpenError::Cancelled)
        ));
    }

    #[test]
    fn aliases_are_distinct_and_tokens_never_wrap() {
        let mut opens = Opens::default();
        let (a, _) = opens.begin("A.fa".into(), String::new(), false).unwrap();
        let (alias, _) = opens.begin("./A.fa".into(), String::new(), false).unwrap();
        assert_ne!(a, alias);
        opens.next = u64::MAX;
        assert!(matches!(
            opens.begin("C.fa".into(), String::new(), false),
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
