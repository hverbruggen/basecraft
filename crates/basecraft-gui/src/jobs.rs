//! The desktop app's one way to run slow work away from the window thread.
//!
//! Work runs on its own thread with a [`Work`] handle: a cancel flag and a
//! progress reporter. Its result arrives once, as a queued call on the window
//! thread of the object that started it. When that object is gone (its tab
//! closed), the result is dropped. Listed jobs (plugin runs) also tell every
//! [`qobject::AppJobs`] when they start, report progress and finish, so the
//! Tasks dialog and status bar update without a timer.
use basecraft_ops::{Session, work::Work};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
};

#[cxx_qt::bridge]
pub mod qobject {
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, revision)]
        type AppJobs = super::JobList;
    }
    impl cxx_qt::Threading for AppJobs {}
    impl cxx_qt::Initialize for AppJobs {}
}

/// Counts changes to the listed jobs; QML refreshes job views when it moves.
#[derive(Default)]
pub struct JobList {
    revision: i32,
}

impl cxx_qt::Initialize for qobject::AppJobs {
    fn initialize(self: Pin<&mut Self>) {
        use cxx_qt::Threading;
        let thread = self.qt_thread();
        watch(move || {
            thread
                .queue(|mut jobs| {
                    let next = jobs.revision().wrapping_add(1);
                    jobs.as_mut().set_revision(next);
                })
                .is_ok()
        });
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

type Watcher = Box<dyn Fn() -> bool + Send>;
static WATCHERS: Mutex<Vec<Watcher>> = Mutex::new(Vec::new());

/// Call `notify` whenever a listed job starts, progresses or finishes, until
/// it returns false.
pub(crate) fn watch(notify: impl Fn() -> bool + Send + 'static) {
    WATCHERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Box::new(notify));
}

fn changed() {
    WATCHERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|notify| notify());
}

const INTERRUPTED: &str = "Interrupted by an internal failure";

/// The one place the desktop app starts a thread. `body` runs once.
pub(crate) fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(body)
        .map(|_| ())
}

fn run<T, R: Send + 'static>(
    owner: impl Owner<T>,
    listed: bool,
    work: impl FnOnce(&Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Result<R, String>) + Send + 'static,
) -> std::io::Result<Work> {
    let handle = if listed {
        Work::new(|_| changed())
    } else {
        Work::default()
    };
    let worker = handle.clone();
    let started = spawn("basecraft-job", move || {
        let result = catch_unwind(AssertUnwindSafe(|| work(&worker)))
            .unwrap_or_else(|_| Err(INTERRUPTED.into()));
        owner.deliver(move |object| done(object, result));
        if listed {
            changed();
        }
    });
    if listed {
        changed();
    }
    started.map(|()| handle)
}

/// Run `work` off the window thread; `done` gets its result on the window
/// thread of `owner`. A panic in `work` arrives as an error.
pub(crate) fn start<T, R: Send + 'static>(
    owner: impl Owner<T>,
    work: impl FnOnce(&Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Result<R, String>) + Send + 'static,
) -> std::io::Result<Work> {
    run(owner, false, work, done)
}

/// [`start`] for a plugin run that appears in the job views.
pub(crate) fn start_listed<T, R: Send + 'static>(
    owner: impl Owner<T>,
    work: impl FnOnce(&Work) -> Result<R, String> + Send + 'static,
    done: impl FnOnce(Pin<&mut T>, Result<R, String>) + Send + 'static,
) -> std::io::Result<Work> {
    run(owner, true, work, done)
}

/// Run `task` with the document's Session moved off the window thread. The
/// Session comes back to `done` on the window thread, also when the task
/// panics or the thread cannot start. Callers refuse edits while it is away.
/// `listed` jobs appear in the job views.
pub(crate) fn with_session<T, R: Send + 'static>(
    owner: impl Owner<T>,
    listed: bool,
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
        listed,
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
            false,
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
    fn listed_jobs_notify_watchers_until_they_leave() {
        let seen = Arc::new(AtomicUsize::new(0));
        let count = seen.clone();
        watch(move || count.fetch_add(1, Ordering::Relaxed) < 100);
        let (owner, queue) = window();
        let temp = tempfile::tempdir().unwrap();
        let mut session = Some(Session::unlogged(temp.path()));
        with_session(
            owner,
            true,
            &mut session,
            |_, work| {
                work.progress("half");
                Ok(())
            },
            |_: Pin<&mut Probe>, _, _| {},
        )
        .unwrap();
        queue.recv_timeout(Duration::from_secs(5)).unwrap()(&mut Probe::default());
        // Start, progress and finish; other tests may add their own.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while seen.load(Ordering::Relaxed) < 3 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
