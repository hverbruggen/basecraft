//! The desktop app's one way to run slow work away from the window thread.
//!
//! Work runs on its own thread with a [`Work`] handle: a cancel flag and a
//! progress reporter. Its result arrives once, as a queued call on the window
//! thread of the object that started it. When that object is gone (its tab
//! closed), the result is dropped. Listed jobs (plugin runs and Sanger stages)
//! also stay in this session's job list, which [`qobject::AppJobs`] shows: the
//! status bar counts and the Tasks dialog bind to it.
use basecraft_ops::{Session, work::Work};
use cxx_qt_lib::QString;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, jobs)]
        #[qproperty(i32, running)]
        #[qproperty(i32, failed)]
        type AppJobs = super::JobViews;
        #[qinvokable]
        fn cancel(self: &AppJobs, id: i32);
    }
    impl cxx_qt::Threading for AppJobs {}
    impl cxx_qt::Initialize for AppJobs {}
}

/// This session's listed jobs as the views show them.
pub struct JobViews {
    jobs: QString,
    running: i32,
    failed: i32,
}
impl Default for JobViews {
    fn default() -> Self {
        Self {
            jobs: QString::from("[]"),
            running: 0,
            failed: 0,
        }
    }
}

impl cxx_qt::Initialize for qobject::AppJobs {
    fn initialize(self: Pin<&mut Self>) {
        use cxx_qt::Threading;
        let thread = self.qt_thread();
        watch(move || thread.queue(qobject::AppJobs::show).is_ok());
        self.show();
    }
}

impl qobject::AppJobs {
    fn show(mut self: Pin<&mut Self>) {
        let list = listed();
        let count = |status: &str| {
            i32::try_from(list.iter().filter(|e| e.status == status).count()).unwrap_or(i32::MAX)
        };
        let (running, failed) = (count("running") + count("cancelling"), count("failed"));
        let text = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
        self.as_mut().set_running(running);
        self.as_mut().set_failed(failed);
        self.set_jobs(QString::from(&text));
    }
    fn cancel(&self, id: i32) {
        cancel(u64::try_from(id).unwrap_or(u64::MAX));
    }
}

/// The window thread of the object a job reports back to.
pub(crate) trait Owner<T>: Clone + Send + 'static {
    /// Queue `call` on the window thread; false when the object is gone.
    fn deliver(&self, call: impl FnOnce(Pin<&mut T>) + Send + 'static) -> bool;
}

impl<T: cxx_qt::Threading + 'static> Owner<T> for cxx_qt::CxxQtThread<T> {
    fn deliver(&self, call: impl FnOnce(Pin<&mut T>) + Send + 'static) -> bool {
        self.queue(call).is_ok()
    }
}

/// How a listed job is named in the job list.
pub(crate) struct Listing {
    pub operation: String,
    pub source: String,
}

/// What a finished listed job left: an error, or the file it made.
#[derive(Default)]
pub(crate) struct Ended {
    pub error: Option<String>,
    pub result: String,
}

/// A result type that a listed job returns.
pub(crate) trait Outcome {
    fn ended(&self) -> Ended;
}

impl Outcome for basecraft_protocol::operation::OperationOutcome {
    fn ended(&self) -> Ended {
        Ended {
            error: (self.state != basecraft_protocol::operation::RuntimeState::Completed)
                .then(|| crate::document_jobs::failure(self)),
            result: self
                .artifacts
                .first()
                .map(|artifact| artifact.path.clone())
                .unwrap_or_default(),
        }
    }
}

/// Provenance actions report a text; finishing is success.
impl Outcome for String {
    fn ended(&self) -> Ended {
        Ended::default()
    }
}

#[derive(Clone, serde::Serialize)]
struct Source {
    path: String,
}

/// One row of the job list, in the shape the Tasks dialog reads.
#[derive(Clone, serde::Serialize)]
pub(crate) struct Entry {
    id: u64,
    operation: String,
    source: Source,
    pub status: &'static str,
    pub progress: String,
    diagnostics: String,
    primary_output: String,
    cancellable: bool,
    #[serde(skip)]
    work: Work,
}

struct Board {
    next: u64,
    entries: Vec<Entry>,
}
static BOARD: Mutex<Board> = Mutex::new(Board {
    next: 0,
    entries: Vec::new(),
});
/// A watcher queues onto a window thread, which may be waiting for one of
/// these locks, so no lock is held while a watcher runs.
type Watcher = Arc<dyn Fn() -> bool + Send + Sync>;
static WATCHERS: Mutex<Vec<Watcher>> = Mutex::new(Vec::new());

fn board() -> std::sync::MutexGuard<'static, Board> {
    BOARD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Call `notify` whenever the job list changes, until it returns false.
pub(crate) fn watch(notify: impl Fn() -> bool + Send + Sync + 'static) {
    WATCHERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Arc::new(notify));
}

/// Call every watcher without holding a lock; forget those that are gone.
fn notify_watchers() {
    let watchers = WATCHERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let gone: Vec<_> = watchers.into_iter().filter(|notify| !notify()).collect();
    if !gone.is_empty() {
        WATCHERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|watcher| !gone.iter().any(|dead| Arc::ptr_eq(dead, watcher)));
    }
}

/// Change the entry `id` and tell the watchers.
fn update(id: u64, change: impl FnOnce(&mut Entry)) {
    if let Some(entry) = board().entries.iter_mut().find(|e| e.id == id) {
        change(entry);
    }
    notify_watchers();
}

impl Entry {
    #[cfg(test)]
    pub(crate) fn operation_is(&self, operation: &str) -> bool {
        self.operation == operation
    }
}

/// This session's listed jobs, oldest first.
pub(crate) fn listed() -> Vec<Entry> {
    board().entries.clone()
}

pub(crate) fn cancel(id: u64) {
    update(id, |entry| {
        if entry.status == "running" {
            entry.work.cancel();
            entry.status = "cancelling";
            entry.cancellable = false;
        }
    });
}

/// Add a running entry; its work reports progress into the entry.
fn list(listing: Listing) -> (u64, Work) {
    let id = {
        let mut board = board();
        board.next += 1;
        board.next
    };
    let work = Work::new(move |text| {
        let text = text.to_owned();
        update(id, |entry| entry.progress = text);
    });
    let entry = Entry {
        id,
        operation: listing.operation,
        source: Source {
            path: listing.source,
        },
        status: "running",
        progress: String::new(),
        diagnostics: String::new(),
        primary_output: String::new(),
        cancellable: true,
        work: work.clone(),
    };
    board().entries.push(entry);
    update(id, |_| {});
    (id, work)
}

fn finish(id: u64, cancelled: bool, ended: Ended) {
    update(id, |entry| {
        entry.cancellable = false;
        entry.status = match (&ended.error, cancelled) {
            (_, true) => "cancelled",
            (Some(_), false) => "failed",
            (None, false) => "completed",
        };
        entry.diagnostics = ended.error.unwrap_or_default();
        entry.primary_output = ended.result;
    });
}

const INTERRUPTED: &str = "Interrupted by an internal failure";

/// The one place the desktop app starts a thread. `body` runs once.
pub(crate) fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(body)
        .map(|_| ())
}

type Summary<R> = Box<dyn Fn(&Result<R, String>) -> Ended + Send>;
/// A session job's summary sees the returned session beside the result.
type SessionSummary<R> = Summary<(Session, Result<R, String>)>;

fn run<T, R: Send + 'static>(
    owner: impl Owner<T>,
    listing: Option<(Listing, Summary<R>)>,
    work: impl FnOnce(&Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Result<R, String>) + Send + 'static,
) -> std::io::Result<Work> {
    let (entry, handle, summary) = match listing {
        Some((listing, summary)) => {
            let (id, work) = list(listing);
            (Some(id), work, Some(summary))
        }
        None => (None, Work::default(), None),
    };
    let worker = handle.clone();
    let started = spawn("basecraft-job", move || {
        let result = catch_unwind(AssertUnwindSafe(|| work(&worker)))
            .unwrap_or_else(|_| Err(INTERRUPTED.into()));
        if let (Some(id), Some(summary)) = (entry, &summary) {
            finish(id, worker.cancelled(), summary(&result));
        }
        owner.deliver(move |object| done(object, result));
    });
    if let (Err(error), Some(id)) = (&started, entry) {
        finish(
            id,
            false,
            Ended {
                error: Some(error.to_string()),
                result: String::new(),
            },
        );
    }
    started.map(|()| handle)
}

fn summarize<R: Outcome>(result: &Result<R, String>) -> Ended {
    match result {
        Ok(value) => value.ended(),
        Err(error) => Ended {
            error: Some(error.clone()),
            result: String::new(),
        },
    }
}

/// Run `work` off the window thread; `done` gets its result on the window
/// thread of `owner`. A panic in `work` arrives as an error.
pub(crate) fn start<T, R: Send + 'static>(
    owner: impl Owner<T>,
    work: impl FnOnce(&Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Result<R, String>) + Send + 'static,
) -> std::io::Result<Work> {
    run(owner, None, work, done)
}

/// [`start`] for a job that appears in the job list.
pub(crate) fn start_listed<T, R: Outcome + Send + 'static>(
    owner: impl Owner<T>,
    listing: Listing,
    work: impl FnOnce(&Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Result<R, String>) + Send + 'static,
) -> std::io::Result<Work> {
    run(owner, Some((listing, Box::new(summarize::<R>))), work, done)
}

/// Run `task` with the document's Session moved off the window thread. The
/// Session comes back to `done` on the window thread, also when the task
/// panics or the thread cannot start. Callers refuse edits while it is away.
pub(crate) fn with_session<T, R: Send + 'static>(
    owner: impl Owner<T>,
    session: &mut Option<Session>,
    task: impl FnOnce(&mut Session, &Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Session, Result<R, String>) + Send + 'static,
) -> Result<Work, String> {
    session_job(owner, None, session, task, done)
}

/// [`with_session`] for a job that appears in the job list.
pub(crate) fn with_session_listed<T, R: Outcome + Send + 'static>(
    owner: impl Owner<T>,
    listing: Listing,
    session: &mut Option<Session>,
    task: impl FnOnce(&mut Session, &Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Session, Result<R, String>) + Send + 'static,
) -> Result<Work, String> {
    let summary: SessionSummary<R> = Box::new(|result| match result {
        Ok((_, result)) => summarize(result),
        Err(error) => summarize::<R>(&Err(error.clone())),
    });
    session_job(owner, Some((listing, summary)), session, task, done)
}

fn session_job<T, R: Send + 'static>(
    owner: impl Owner<T>,
    listing: Option<(Listing, SessionSummary<R>)>,
    session: &mut Option<Session>,
    task: impl FnOnce(&mut Session, &Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Session, Result<R, String>) + Send + 'static,
) -> Result<Work, String> {
    let slot =
        Arc::new(Mutex::new(Some(session.take().ok_or(
            "Document is unavailable while another operation owns it",
        )?)));
    let worker_slot = slot.clone();
    let started = run(
        owner,
        listing,
        move |work| {
            let mut session = worker_slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .ok_or(INTERRUPTED)?;
            let result = catch_unwind(AssertUnwindSafe(|| task(&mut session, work)))
                .unwrap_or_else(|_| Err(INTERRUPTED.into()));
            Ok((session, result))
        },
        move |object, result| {
            if let Ok((session, result)) = result {
                done(object, session, result);
            }
        },
    );
    started.map_err(|error| {
        *session = slot.lock().unwrap_or_else(PoisonError::into_inner).take();
        error.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;

    /// A stand-in window: queued calls run when the test thread drains them.
    #[derive(Default)]
    struct Probe {
        results: Vec<(std::thread::ThreadId, Result<&'static str, String>)>,
    }
    type Call = Box<dyn FnOnce(&mut Probe) + Send>;
    #[derive(Clone)]
    struct Window {
        calls: mpsc::Sender<Call>,
        open: Arc<AtomicBool>,
    }
    impl Owner<Probe> for Window {
        fn deliver(&self, call: impl FnOnce(Pin<&mut Probe>) + Send + 'static) -> bool {
            self.open.load(Ordering::Acquire)
                && self
                    .calls
                    .send(Box::new(move |probe| call(Pin::new(probe))))
                    .is_ok()
        }
    }
    fn window() -> (Window, mpsc::Receiver<Call>) {
        let (calls, queue) = mpsc::channel();
        let open = Arc::new(AtomicBool::new(true));
        (Window { calls, open }, queue)
    }

    #[test]
    fn cancelled_job_completes_once_on_the_window_thread() {
        let (owner, queue) = window();
        let work = start(
            owner,
            |work| {
                while !work.cancelled() {
                    std::thread::yield_now();
                }
                Ok("stopped")
            },
            |mut probe: Pin<&mut Probe>, result| {
                probe.results.push((std::thread::current().id(), result));
            },
        )
        .unwrap();
        work.cancel();
        let mut probe = Probe::default();
        queue.recv_timeout(Duration::from_secs(5)).unwrap()(&mut probe);
        assert_eq!(
            probe.results,
            [(std::thread::current().id(), Ok("stopped"))]
        );
        assert!(queue.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn closed_owner_receives_nothing_and_panics_become_errors() {
        let (owner, queue) = window();
        let open = owner.open.clone();
        let (release, gate) = mpsc::channel::<()>();
        let delivered = Arc::new(AtomicUsize::new(0));
        let count = delivered.clone();
        start(
            owner.clone(),
            move |_| {
                gate.recv().unwrap();
                Ok("late")
            },
            move |_: Pin<&mut Probe>, _| {
                count.fetch_add(1, Ordering::Relaxed);
            },
        )
        .unwrap();
        open.store(false, Ordering::Release);
        release.send(()).unwrap();
        assert!(queue.recv_timeout(Duration::from_millis(500)).is_err());
        assert_eq!(delivered.load(Ordering::Relaxed), 0);

        open.store(true, Ordering::Release);
        start(
            owner,
            |_| -> Result<&'static str, String> { panic!("fixture") },
            |mut probe: Pin<&mut Probe>, result| {
                probe.results.push((std::thread::current().id(), result));
            },
        )
        .unwrap();
        let mut probe = Probe::default();
        queue.recv_timeout(Duration::from_secs(5)).unwrap()(&mut probe);
        assert_eq!(probe.results[0].1, Err(INTERRUPTED.into()));
    }

    #[test]
    fn session_returns_to_the_window_after_a_failed_task() {
        let temp = tempfile::tempdir().unwrap();
        let mut session = Some(Session::unlogged(temp.path()));
        let identity = session.as_ref().unwrap().id().to_owned();
        let (owner, queue) = window();
        let (tx, returned) = mpsc::channel();
        with_session(
            owner,
            &mut session,
            |_, _| -> Result<(), String> { panic!("fixture") },
            move |_: Pin<&mut Probe>, session, result| {
                tx.send((session.id().to_owned(), result)).unwrap();
            },
        )
        .unwrap();
        assert!(
            session.is_none(),
            "the worker owns the session while running"
        );
        queue.recv_timeout(Duration::from_secs(5)).unwrap()(&mut Probe::default());
        assert_eq!(
            returned.recv().unwrap(),
            (identity, Err(INTERRUPTED.into()))
        );
    }

    #[test]
    fn a_listed_job_appears_reports_progress_and_finishes_in_the_list() {
        let seen = Arc::new(AtomicUsize::new(0));
        let count = seen.clone();
        watch(move || count.fetch_add(1, Ordering::Relaxed) < 1000);
        let (owner, queue) = window();
        let (release, gate) = mpsc::channel::<()>();
        let (reported, progress) = mpsc::channel::<()>();
        let label = "fixture listing";
        let work = start_listed(
            owner,
            Listing {
                operation: label.into(),
                source: "/a.afa".into(),
            },
            move |work| {
                work.progress("half");
                reported.send(()).unwrap();
                gate.recv().unwrap();
                Ok::<_, String>(String::from("done"))
            },
            |_: Pin<&mut Probe>, _| {},
        )
        .unwrap();
        let row = || listed().into_iter().find(|e| e.operation == label).unwrap();
        progress.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!((row().status, row().progress.as_str()), ("running", "half"));
        cancel(row().id);
        assert!(work.cancelled(), "cancelling the row cancels the work");
        assert_eq!(row().status, "cancelling");
        release.send(()).unwrap();
        queue.recv_timeout(Duration::from_secs(5)).unwrap()(&mut Probe::default());
        assert_eq!(row().status, "cancelled");
        assert!(!row().cancellable);
        assert!(
            seen.load(Ordering::Relaxed) >= 4,
            "start, progress, cancel, finish"
        );
    }

    #[test]
    fn a_failed_listed_job_keeps_its_error() {
        let (owner, queue) = window();
        start_listed(
            owner,
            Listing {
                operation: "fixture failure".into(),
                source: String::new(),
            },
            |_| Err::<String, _>("no plugin".into()),
            |_: Pin<&mut Probe>, _| {},
        )
        .unwrap();
        queue.recv_timeout(Duration::from_secs(5)).unwrap()(&mut Probe::default());
        let row = listed()
            .into_iter()
            .find(|e| e.operation == "fixture failure")
            .unwrap();
        assert_eq!(
            (row.status, row.diagnostics.as_str()),
            ("failed", "no plugin")
        );
    }
}
