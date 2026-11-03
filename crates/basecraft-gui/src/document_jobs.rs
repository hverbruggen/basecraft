//! Registered operations run on an open document. Each job runs on its own
//! thread; the owning bridge polls it on the Qt thread so the session records
//! the run in the document's history before it reports completion.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use basecraft_ops::{DocId, Session};
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::operation::{InputReference, OperationOutcome, OperationRequest};

pub(crate) struct DocumentJob {
    manager: JobManager,
    pub run_id: String,
}

/// The registered ID of a bundled plugin operation, e.g. `basecraft.mafft/align@1`.
pub(crate) fn bundled(plugin: &str, operation: &str) -> String {
    format!("basecraft.{plugin}/{operation}@1")
}

fn registry() -> Result<Registry, String> {
    RegistryConfig::discover()
        .and_then(|config| Registry::load(&config))
        .map_err(|error| error.to_string())
}

/// The collection member that holds an operation's main result, such as
/// `tree.nwk`, as its descriptor declares it.
pub(crate) fn primary_member(operation: &str) -> Result<String, String> {
    let registry = registry()?;
    let descriptor = &registry
        .resolve(operation, None)
        .map_err(|error| error.to_string())?
        .descriptor;
    let output = descriptor
        .legacy_adapter
        .as_ref()
        .and_then(|adapter| adapter.outputs.get("output"))
        .ok_or_else(|| format!("{operation} declares no primary result"))?;
    Ok(format!("{}/{}", output.role, output.path))
}

/// A destination for the policy rooted at `root`: relative when inside it.
fn destination(root: &Path, path: &Path) -> Result<String, String> {
    let parent = path
        .parent()
        .ok_or("output has no folder")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let name = path.file_name().ok_or("output has no file name")?;
    let path = parent.join(name);
    Ok(path
        .strip_prefix(root)
        .map_or(path.as_path(), |relative| relative)
        .display()
        .to_string())
}

impl DocumentJob {
    /// Submit `operation` with `doc` as its document input. `exports` maps a
    /// result member (`role/member`) to a new file inside the project.
    pub fn submit(
        session: &mut Session,
        doc: DocId,
        operation: &str,
        parameters: serde_json::Value,
        exports: &[(String, PathBuf)],
    ) -> Result<Self, String> {
        let registry = registry()?;
        let input = registry
            .resolve(operation, None)
            .map_err(|error| error.to_string())?
            .descriptor
            .inputs
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| format!("{operation} takes no input"))?;
        let policy = SessionPolicy::new(session.root(), true).map_err(|error| error.to_string())?;
        let root = policy.root.clone();
        let destinations = exports
            .iter()
            .map(|(member, path)| Ok((member.clone(), destination(&root, path)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let request = OperationRequest {
            operation: operation.into(),
            inputs: [(
                input,
                vec![InputReference::Document {
                    doc: doc.0.to_string(),
                    generation: session.generation(doc),
                    revision: session.revision(doc),
                }],
            )]
            .into(),
            parameters,
            destinations,
            pins: None,
            request_key: None,
            target: None,
        };
        let mut manager = JobManager::default().with_task_cancellation();
        let accepted = manager
            .submit_in_session(&registry, request, policy, "GUI", session)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            manager,
            run_id: accepted.run_id,
        })
    }

    /// Submit `operation` on project files with `target` as the request's
    /// target: a species workspace analysing a file it references receives
    /// the run in its history and its hypotheses.
    pub fn submit_files(
        session: &mut Session,
        target: DocId,
        operation: &str,
        inputs: BTreeMap<String, Vec<PathBuf>>,
        parameters: serde_json::Value,
        exports: &[(String, PathBuf)],
    ) -> Result<Self, String> {
        let registry = registry()?;
        let policy = SessionPolicy::new(session.root(), true).map_err(|error| error.to_string())?;
        let root = policy.root.clone();
        let destinations = exports
            .iter()
            .map(|(member, path)| Ok((member.clone(), destination(&root, path)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let inputs = inputs
            .into_iter()
            .map(|(role, paths)| {
                let paths = paths
                    .iter()
                    .map(|path| {
                        Ok(InputReference::File {
                            path: destination(&root, path)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok((role, paths))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let request = OperationRequest {
            operation: operation.into(),
            inputs,
            parameters,
            destinations,
            pins: None,
            request_key: None,
            target: Some(InputReference::Document {
                doc: target.0.to_string(),
                generation: session.generation(target),
                revision: session.revision(target),
            }),
        };
        let mut manager = JobManager::default().with_task_cancellation();
        let accepted = manager
            .submit_in_session(&registry, request, policy, "GUI", session)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            manager,
            run_id: accepted.run_id,
        })
    }

    /// Let the session record finished work; the outcome once it is terminal.
    pub fn poll(&self, session: &mut Session) -> Result<Option<OperationOutcome>, String> {
        self.manager.poll_session(session);
        let outcome = self
            .manager
            .get(&self.run_id)
            .map_err(|error| error.to_string())?;
        Ok(outcome.state.terminal().then_some(outcome))
    }

    pub fn finalized_generation(&self) -> Option<u64> {
        self.manager.finalized_generation(&self.run_id)
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.manager
            .cancel(&self.run_id)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// The progress file the plugin updates while it runs.
    pub fn progress_file(&self) -> Option<PathBuf> {
        let outcome = self.manager.get(&self.run_id).ok()?;
        outcome
            .evidence
            .map(|evidence| PathBuf::from(evidence).join("progress"))
    }
}

/// The parameters an operation's descriptor declares; others are refused.
pub(crate) fn declared_parameters(
    operation: &str,
) -> Result<std::collections::BTreeSet<String>, String> {
    let registry = registry()?;
    let descriptor = &registry
        .resolve(operation, None)
        .map_err(|error| error.to_string())?
        .descriptor;
    Ok(descriptor
        .parameters
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default())
}

/// The input an operation reads `path` through: its only input, or the one
/// whose view matches a tree or an alignment file.
pub(crate) fn input_role(operation: &str, path: &Path) -> Result<String, String> {
    use basecraft_protocol::operation::InputView;
    let registry = registry()?;
    let inputs = &registry
        .resolve(operation, None)
        .map_err(|error| error.to_string())?
        .descriptor
        .inputs;
    let tree = basecraft_ops::files::Format::from_path(path).ok()
        == Some(basecraft_ops::files::Format::Tree);
    inputs
        .iter()
        .find(|(_, binding)| {
            inputs.len() == 1
                || (tree && binding.view == InputView::Newick)
                || (!tree && binding.view == InputView::AlignedFasta)
        })
        .map(|(role, _)| role.clone())
        .ok_or_else(|| format!("{operation} cannot read {}", path.display()))
}

/// A retained result file of a completed run, by file name.
pub(crate) fn result_file(outcome: &OperationOutcome, name: &str) -> Option<PathBuf> {
    outcome
        .artifacts
        .iter()
        .map(|artifact| PathBuf::from(&artifact.path))
        .find(|path| path.file_name().is_some_and(|file| file == name))
}

/// The first error message of a terminal outcome that did not complete.
pub(crate) fn failure(outcome: &OperationOutcome) -> String {
    if outcome.state == basecraft_protocol::operation::RuntimeState::Cancelled {
        return "Cancelled".into();
    }
    outcome.errors.first().map_or_else(
        || "Operation did not complete".into(),
        |error| crate::bridge::error_summary(&error.message),
    )
}

/// Transfer a document owner to a background continuation, including runner
/// preparation and owner-session finalization. Failed thread creation returns
/// the untouched session; a caught task panic also returns ownership. GUI
/// bridges must reject replacement/edits until the session is returned.
pub(crate) fn background_session<T: Send + 'static>(
    session: Session,
    task: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
    completed: impl FnOnce(Session, Result<T, String>) + Send + 'static,
) -> Result<(), (Box<Session>, String)> {
    let owner = std::sync::Arc::new(std::sync::Mutex::new(Some(session)));
    let worker_owner = owner.clone();
    let spawned = std::thread::Builder::new()
        .name("document-operation".into())
        .spawn(move || {
            let Some(mut session) = worker_owner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            else {
                return;
            };
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(&mut session)))
                    .unwrap_or_else(|_| {
                        Err("Document operation interrupted by an internal failure".into())
                    });
            completed(session, result);
        });
    if let Err(error) = spawned
        && let Some(session) = owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    {
        return Err((Box::new(session), error.to_string()));
    }
    Ok(())
}

/// Qt transport for the existing document runner. The owning Session moves to
/// its worker for preparation, execution and finalization, then returns intact
/// through poll. No operation state or publication logic is reimplemented here.
type ReturnedDocument = (Session, Result<(OperationOutcome, Option<u64>), String>);
pub(crate) struct BackgroundDocumentJob {
    receiver: std::sync::mpsc::Receiver<ReturnedDocument>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
    generation: std::cell::Cell<Option<u64>>,
    pub started: Instant,
}
impl Drop for BackgroundDocumentJob {
    fn drop(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
impl BackgroundDocumentJob {
    fn start(
        session: &mut Option<Session>,
        prepare: impl FnOnce(&mut Session) -> Result<DocumentJob, String> + Send + 'static,
    ) -> Result<Self, String> {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let owned = session
            .take()
            .ok_or("Document is unavailable while another operation owns it")?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let progress = Arc::new(Mutex::new(None));
        let worker_progress = progress.clone();
        let (sender, receiver) = mpsc::channel();
        let spawned = background_session(
            owned,
            move |session| {
                if flag.load(Ordering::Acquire) {
                    return Err("Operation cancelled before preparation".into());
                }
                let job = prepare(session)?;
                *worker_progress
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = job.progress_file();
                loop {
                    if flag.load(Ordering::Acquire) {
                        job.cancel()?;
                    }
                    if let Some(outcome) = job.poll(session)? {
                        return Ok((outcome, job.finalized_generation()));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            },
            move |session, result| {
                let _ = sender.send((session, result));
            },
        );
        if let Err((owned, error)) = spawned {
            *session = Some(*owned);
            return Err(error);
        }
        Ok(Self {
            receiver,
            cancelled,
            progress,
            generation: std::cell::Cell::new(None),
            started: Instant::now(),
        })
    }
    pub fn submit(
        session: &mut Option<Session>,
        doc: DocId,
        operation: &str,
        parameters: serde_json::Value,
        exports: &[(String, PathBuf)],
    ) -> Result<Self, String> {
        let operation = operation.to_owned();
        let exports = exports.to_vec();
        Self::start(session, move |session| {
            DocumentJob::submit(session, doc, &operation, parameters, &exports)
        })
    }
    pub fn submit_files(
        session: &mut Option<Session>,
        doc: DocId,
        operation: &str,
        inputs: BTreeMap<String, Vec<PathBuf>>,
        parameters: serde_json::Value,
        exports: &[(String, PathBuf)],
    ) -> Result<Self, String> {
        let operation = operation.to_owned();
        let exports = exports.to_vec();
        Self::start(session, move |session| {
            DocumentJob::submit_files(session, doc, &operation, inputs, parameters, &exports)
        })
    }
    pub fn poll(&self, session: &mut Option<Session>) -> Result<Option<OperationOutcome>, String> {
        match self.receiver.try_recv() {
            Ok((owned, result)) => {
                if session.is_some() {
                    return Err("Document owner replaced while an operation was running".into());
                }
                *session = Some(owned);
                let (outcome, generation) = result?;
                self.generation.set(generation);
                Ok(Some(outcome))
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("Document operation worker disconnected".into())
            }
        }
    }
    pub fn finalized_generation(&self) -> Option<u64> {
        self.generation.get()
    }
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn progress_file(&self) -> Option<PathBuf> {
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod background_tests {
    use super::*;

    #[test]
    fn background_session_returns_ownership_after_failure_without_blocking_submission() {
        let temp = tempfile::tempdir().unwrap();
        let session = Session::unlogged(temp.path());
        let identity = session.id().to_owned();
        let (gate, wait) = std::sync::mpsc::channel();
        let (complete, result) = std::sync::mpsc::channel();
        assert!(
            background_session(
                session,
                move |_| {
                    wait.recv_timeout(std::time::Duration::from_secs(5))
                        .map_err(|e| e.to_string())?;
                    Err::<(), _>("fixture failure".into())
                },
                move |session, outcome| {
                    complete.send((session, outcome)).unwrap();
                }
            )
            .is_ok()
        );
        assert!(
            result.try_recv().is_err(),
            "submission must return before the task completes"
        );
        gate.send(()).unwrap();
        let (session, outcome) = result
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(session.id(), identity);
        assert_eq!(outcome.unwrap_err(), "fixture failure");
    }

    #[test]
    fn background_document_job_returns_same_session_after_registered_finalization() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("sequence.gb");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sequences-annotations/locus.gb"),
            &file,
        )
        .unwrap();
        let mut owned = Session::unlogged(temp.path());
        let doc = owned.open(&file).unwrap();
        let identity = owned.id().to_owned();
        let mut session = Some(owned);
        let job = BackgroundDocumentJob::submit(
            &mut session,
            doc,
            "basecraft.orf-finder/find_orfs@1",
            serde_json::json!({"min_codons":1}),
            &[],
        )
        .unwrap();
        assert!(
            session.is_none(),
            "worker exclusively owns document while running"
        );
        let started = Instant::now();
        let outcome = loop {
            if let Some(outcome) = job.poll(&mut session).unwrap() {
                break outcome;
            }
            assert!(started.elapsed().as_secs() < 30);
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(
            outcome.state,
            basecraft_protocol::operation::RuntimeState::Completed,
            "{outcome:?}"
        );
        assert_eq!(session.as_ref().unwrap().id(), identity);
        assert!(session.as_ref().unwrap().checked_sequence(doc).is_ok());
    }
}
