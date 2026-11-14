//! Document-open steps the bootstrap test blocks or fails: one held path
//! whose worker waits at a gate, per-path failures, and the documents each
//! editor installed with the reads it took to open them.
use crate::app_bridge::{OPEN_HOOKS, OpenHooks};
use crate::document_open::Candidate;
use basecraft_io::counters::Counters;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

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
struct Flag {
    set: Mutex<bool>,
    changed: Condvar,
}

impl Flag {
    fn raise(&self) {
        *self.set.lock().unwrap() = true;
        self.changed.notify_all();
    }

    fn wait(&self, what: &str) {
        let set = self.set.lock().unwrap();
        let (set, timeout) = self
            .changed
            .wait_timeout_while(set, Duration::from_secs(10), |set| !*set)
            .unwrap();
        assert!(*set && !timeout.timed_out(), "{what} timed out");
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
    faults: BTreeMap<String, (Fault, bool)>,
    /// What each worker read, until its document is installed.
    reads: BTreeMap<PathBuf, Counters>,
    admissions: Vec<(PathBuf, Counters)>,
}

struct TestOpens(Mutex<State>);

static HOOKS: TestOpens = TestOpens(Mutex::new(State {
    held: None,
    faults: BTreeMap::new(),
    reads: BTreeMap::new(),
    admissions: Vec::new(),
}));

/// Route every document open in this process through these hooks.
pub fn install() {
    assert!(OPEN_HOOKS.set(&HOOKS).is_ok(), "open hooks installed twice");
}

fn state() -> std::sync::MutexGuard<'static, State> {
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
    let (released, finished) = {
        let state = state();
        let held = state.held.as_ref().expect("a held open");
        (held.released.clone(), held.finished.clone())
    };
    released.raise();
    finished.wait("held worker after shell destruction");
}

/// Make the next open of `path` fail with `fault`.
pub fn fail(path: String, fault: Fault) {
    state().faults.insert(path, (fault, false));
}

/// The documents installed since the last call, with their reads.
pub fn take_admissions() -> Vec<(PathBuf, Counters)> {
    std::mem::take(&mut state().admissions)
}

impl TestOpens {
    fn take_fault(&self, path: &str, fault: Fault) -> bool {
        let mut state = self.0.lock().unwrap();
        let found = state.faults.get(path).is_some_and(|(f, _)| *f == fault);
        if found {
            state.faults.remove(path);
        }
        found
    }

    fn release_if(&self, matches: impl Fn(&Held) -> bool) {
        if let Some(held) = self.0.lock().unwrap().held.as_ref()
            && matches(held)
        {
            held.released.raise();
        }
    }
}

impl OpenHooks for TestOpens {
    fn spawn(&self, path: &str, work: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
        if self.take_fault(path, Fault::Spawn) {
            return Err(std::io::Error::other("injected spawn failure"));
        }
        std::thread::Builder::new()
            .name("document-open".into())
            .spawn(work)
            .map(|_| ())
    }

    fn worker(&self, path: &str) {
        let gate = {
            let mut state = self.0.lock().unwrap();
            state
                .held
                .as_mut()
                .filter(|held| !held.handed_out && held.path == path)
                .map(|held| {
                    held.handed_out = true;
                    held.released.clone()
                })
        };
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
        thread: &cxx_qt::CxxQtThread<crate::app_bridge::qobject::DocumentOpens>,
        call: Box<dyn FnOnce(std::pin::Pin<&mut crate::app_bridge::qobject::DocumentOpens>) + Send>,
    ) -> Result<(), cxx_qt::ThreadingQueueError> {
        let failed = {
            let mut state = self.0.lock().unwrap();
            // Keyed as the candidate names its file.
            let file = std::path::Path::new(path);
            let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
            state.reads.insert(file, basecraft_io::counters::snapshot());
            match state.faults.get_mut(path) {
                Some((Fault::Queue, _)) => {
                    state.faults.remove(path);
                    true
                }
                Some((Fault::DoubleQueue, once)) if !*once => {
                    *once = true;
                    true
                }
                Some((Fault::DoubleQueue, _)) => {
                    state.faults.remove(path);
                    true
                }
                _ => false,
            }
        };
        if failed {
            drop(call);
            return Err(cxx_qt::ThreadingQueueError::InvokeMethodFailed);
        }
        thread.queue(call)
    }

    fn finished(&self, path: &str) {
        if let Some(held) = self.0.lock().unwrap().held.as_ref()
            && held.path == path
        {
            held.finished.raise();
        }
    }

    fn installed(&self, candidate: &Candidate) {
        let mut state = self.0.lock().unwrap();
        let reads = state.reads.remove(&candidate.path).unwrap_or_default();
        state.admissions.push((candidate.path.clone(), reads));
    }

    fn ready(&self, path: &str) {
        self.release_if(|held| held.release_path == path);
    }

    fn retired(&self, path: &str) {
        self.release_if(|held| held.path == path);
    }
}
