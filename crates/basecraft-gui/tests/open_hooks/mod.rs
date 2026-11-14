//! Document-open steps the bootstrap test blocks or fails: one held path
//! whose worker waits at a gate, per-path failures, and the documents each
//! editor installed with the reads it took to open them.
use crate::app_bridge::{OPEN_HOOKS, OpenHooks, qobject::DocumentOpens};
use crate::document_open::Candidate;
use basecraft_io::counters::Counters;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};

/// A failure the next open of a path suffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The worker does not start.
    Spawn,
    /// The worker panics before it reads the file.
    Worker,
    /// Handing the result to the window fails; the error still arrives.
    Queue,
    /// Handing over the result and the error both fail.
    DoubleQueue,
}

#[derive(Default)]
struct Flag(Mutex<bool>, Condvar);

impl Flag {
    fn raise(&self) {
        *self.0.lock().unwrap() = true;
        self.1.notify_all();
    }

    fn wait(&self, what: &str) {
        let set = self.0.lock().unwrap();
        let ten_seconds = std::time::Duration::from_secs(10);
        let (set, _) = self
            .1
            .wait_timeout_while(set, ten_seconds, |set| !*set)
            .unwrap();
        assert!(*set, "{what} timed out");
    }
}

struct Held {
    path: String,
    release_path: String,
    handed_out: bool,
    released: Arc<Flag>,
    finished: Arc<Flag>,
}

#[derive(Default)]
struct State {
    held: Option<Held>,
    faults: BTreeMap<String, Fault>,
    /// What each worker read, until its document is installed.
    reads: BTreeMap<PathBuf, Counters>,
    admissions: Vec<(PathBuf, Counters)>,
}

#[derive(Default)]
struct TestOpens(Mutex<State>);

static HOOKS: LazyLock<TestOpens> = LazyLock::new(TestOpens::default);

/// Route every document open in this process through these hooks.
pub fn install() {
    assert!(
        OPEN_HOOKS.set(&*HOOKS).is_ok(),
        "open hooks installed twice"
    );
}

fn state() -> MutexGuard<'static, State> {
    HOOKS.0.lock().unwrap()
}

/// Make the worker opening `path` wait until `release_path` is ready, or
/// until `path`'s own request ends.
pub fn hold(path: String, release_path: String) {
    state().held = Some(Held {
        path,
        release_path,
        handed_out: false,
        released: Arc::default(),
        finished: Arc::default(),
    });
}

/// Release the held worker and wait until it has handed over its result.
pub fn release_and_wait() {
    let (released, finished) = HOOKS.held(|_| true).expect("a held open");
    released.raise();
    finished.wait("held worker after shell destruction");
}

/// Make the next open of `path` fail with `fault`.
pub fn fail(path: String, fault: Fault) {
    state().faults.insert(path, fault);
}

/// The documents installed since the last call, with their reads.
pub fn take_admissions() -> Vec<(PathBuf, Counters)> {
    std::mem::take(&mut state().admissions)
}

impl TestOpens {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }

    /// Whether `path` has `fault` pending; it is used up.
    fn take_fault(&self, path: &str, fault: Fault) -> bool {
        let mut state = self.state();
        let found = state.faults.get(path) == Some(&fault);
        if found {
            state.faults.remove(path);
        }
        found
    }

    fn held(&self, matches: impl Fn(&Held) -> bool) -> Option<(Arc<Flag>, Arc<Flag>)> {
        let state = self.state();
        let held = state.held.as_ref().filter(|held| matches(held))?;
        Some((held.released.clone(), held.finished.clone()))
    }
}

impl OpenHooks for TestOpens {
    fn spawn(&self, path: &str, work: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
        if self.take_fault(path, Fault::Spawn) {
            return Err(std::io::Error::other("injected spawn failure"));
        }
        let path = path.to_owned();
        crate::jobs::spawn("document-open", move || {
            work();
            if let Some((_, finished)) = HOOKS.held(|held| held.path == path) {
                finished.raise();
            }
        })
    }

    fn worker(&self, path: &str) {
        let gate = self.state().held.as_mut().and_then(|held| {
            let take = !held.handed_out && held.path == path;
            held.handed_out |= take;
            take.then(|| held.released.clone())
        });
        if let Some(gate) = gate {
            gate.wait("open test barrier");
        }
        assert!(
            !self.take_fault(path, Fault::Worker),
            "injected worker panic"
        );
        basecraft_io::counters::reset();
    }

    fn deliver(
        &self,
        path: &str,
        thread: &cxx_qt::CxxQtThread<DocumentOpens>,
        call: Box<dyn FnOnce(std::pin::Pin<&mut DocumentOpens>) + Send>,
    ) -> Result<(), cxx_qt::ThreadingQueueError> {
        let failed = {
            let mut state = self.state();
            // Keyed as the candidate names its file.
            let file = Path::new(path)
                .canonicalize()
                .unwrap_or_else(|_| path.into());
            state.reads.insert(file, basecraft_io::counters::snapshot());
            match state.faults.get(path) {
                Some(Fault::Queue) => state.faults.remove(path).is_some(),
                Some(Fault::DoubleQueue) => {
                    state.faults.insert(path.into(), Fault::Queue).is_some()
                }
                _ => false,
            }
        };
        if failed {
            return Err(cxx_qt::ThreadingQueueError::InvokeMethodFailed);
        }
        thread.queue(call)
    }

    fn installed(&self, candidate: &Candidate) {
        let mut state = self.state();
        let reads = state.reads.remove(&candidate.path).unwrap_or_default();
        state.admissions.push((candidate.path.clone(), reads));
    }

    fn ready(&self, path: &str) {
        if let Some((released, _)) = self.held(|held| held.release_path == path) {
            released.raise();
        }
    }

    fn retired(&self, path: &str) {
        if let Some((released, _)) = self.held(|held| held.path == path) {
            released.raise();
        }
    }
}
