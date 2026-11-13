//! Registered operations run on an open document. The run happens on a job
//! thread that owns the document's Session, so the session records the run in
//! the document's history before the window hears of its completion.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_ops::{DocId, Session};
use basecraft_plugin_host::{jobs::JobManager, policy::SessionPolicy};
use basecraft_protocol::operation::{InputReference, OperationOutcome, OperationRequest};

pub(crate) struct DocumentJob {
    manager: JobManager,
    pub run_id: String,
}

/// The registered ID of a bundled plugin operation, e.g. `basecraft.mafft/align@1`.
pub(crate) fn bundled(plugin: &str, operation: &str) -> String {
    format!("basecraft.{plugin}/{operation}@1")
}

/// The collection member that holds an operation's main result, such as
/// `tree.nwk`, as its descriptor declares it.
pub(crate) fn primary_member(operation: &str) -> Result<String, String> {
    crate::plugins::descriptor(operation)?
        .outputs
        .iter()
        .find_map(|(role, output)| Some(format!("{role}/{}", output.primary.as_ref()?)))
        .ok_or_else(|| format!("{operation} declares no primary result"))
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
        let registry = crate::plugins::registry().map_err(|error| error.to_string())?;
        let input = crate::plugins::descriptor(operation)?
            .inputs
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| format!("{operation} takes no input"))?;
        let policy = SessionPolicy::new(session.root(), true).map_err(|error| error.to_string())?;
        let root = policy.root.clone();
        let destinations = exports
            .iter()
            .map(|(member, path)| {
                let path = basecraft_ops::requests::request_path(&root, path)
                    .map_err(|error| error.to_string())?;
                Ok((member.clone(), path))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let request = OperationRequest {
            operation: operation.into(),
            inputs: [(
                input,
                vec![InputReference::Document {
                    doc: doc.0.to_string(),
                    generation: session.generation(doc).map_err(|e| e.to_string())?,
                    revision: session.revision(doc).map_err(|e| e.to_string())?,
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
        inputs: &BTreeMap<String, Vec<PathBuf>>,
        parameters: serde_json::Value,
        exports: &[(String, PathBuf)],
    ) -> Result<Self, String> {
        let registry = crate::plugins::registry().map_err(|error| error.to_string())?;
        let policy = SessionPolicy::new(session.root(), true).map_err(|error| error.to_string())?;
        // The same request the CLI's `run` makes for files, aimed at `target`.
        let outputs = exports.iter().cloned().collect();
        let mut request = basecraft_ops::requests::file_request(
            &policy.root,
            operation,
            inputs,
            parameters,
            &outputs,
        )
        .map_err(|error| error.to_string())?;
        request.target = Some(InputReference::Document {
            doc: target.0.to_string(),
            generation: session.generation(target).map_err(|e| e.to_string())?,
            revision: session.revision(target).map_err(|e| e.to_string())?,
        });
        let mut manager = JobManager::default().with_task_cancellation();
        let accepted = manager
            .submit_in_session(&registry, request, policy, "GUI", session)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            manager,
            run_id: accepted.run_id,
        })
    }

    /// Wait on the session's thread until the run is terminal; the session
    /// records it on the way. Cancelling `work` cancels the run. `progress`
    /// gets each new record from the plugin's progress file.
    pub fn wait(
        &self,
        session: &mut Session,
        work: &basecraft_ops::work::Work,
        progress: impl FnMut(serde_json::Value),
    ) -> Result<OperationOutcome, String> {
        let file = self.progress_file();
        let state = std::cell::RefCell::new((0u8, None, progress));
        self.manager
            .wait_in_session(&self.run_id, session, &|| {
                let mut state = state.borrow_mut();
                let (tick, shown, progress) = &mut *state;
                *tick = tick.wrapping_add(1);
                // The plugin rewrites its progress file a few times a second.
                if *tick % 8 == 0
                    && let Some(value) = file.as_deref().and_then(read_progress)
                    && shown.as_ref() != Some(&value)
                {
                    *shown = Some(value.clone());
                    progress(value);
                }
                work.cancelled()
            })
            .map_err(|error| error.to_string())
    }

    pub fn finalized_generation(&self) -> Option<u64> {
        self.manager.finalized_generation(&self.run_id)
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
    Ok(crate::plugins::descriptor(operation)?
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
    let inputs = &crate::plugins::descriptor(operation)?.inputs;
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

/// A finished plugin run and the document generation the session recorded it at.
pub(crate) struct Finished {
    pub outcome: OperationOutcome,
    pub generation: Option<u64>,
}

/// Read the progress file a plugin keeps up to date while it runs.
fn read_progress(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Run a registered operation on the document's Session off the window
/// thread. `progress` receives each new progress record and `done` the
/// outcome, both on the window thread of `owner`; `done` also gets the
/// Session back. Cancelling the returned [`Work`] cancels the run.
pub(crate) fn run_operation<T: 'static>(
    owner: impl crate::jobs::Owner<T>,
    listing: crate::jobs::Listing,
    session: &mut Option<Session>,
    prepare: impl FnOnce(&mut Session) -> Result<DocumentJob, String> + Send + 'static,
    progress: fn(std::pin::Pin<&mut T>, &serde_json::Value),
    done: impl FnOnce(std::pin::Pin<&mut T>, Session, Result<Finished, String>) + Send + 'static,
) -> Result<basecraft_ops::work::Work, String> {
    let reporter = owner.clone();
    crate::jobs::with_session_listed(
        owner,
        listing,
        session,
        move |session, work| {
            if work.cancelled() {
                return Err("Operation cancelled before preparation".into());
            }
            let job = prepare(session)?;
            let outcome = job.wait(session, work, |value| {
                work.progress(value["phase"].as_str().unwrap_or_default());
                reporter.deliver(move |owner| progress(owner, &value));
            })?;
            Ok(Finished {
                outcome,
                generation: job.finalized_generation(),
            })
        },
        done,
    )
}

impl crate::jobs::Outcome for Finished {
    fn ended(&self) -> crate::jobs::Ended {
        self.outcome.ended()
    }
}

#[cfg(test)]
#[allow(dead_code)] // harness=false application_bootstrap omits the test bodies.
mod tests {
    #[allow(unused_imports)] // harness=false application_bootstrap includes this module.
    use super::*;
    use std::sync::mpsc;

    struct Probe;
    type Call = Box<dyn FnOnce(std::pin::Pin<&mut Probe>) + Send>;
    #[derive(Clone)]
    struct Window(mpsc::Sender<Call>);
    impl crate::jobs::Owner<Probe> for Window {
        fn deliver(&self, call: impl FnOnce(std::pin::Pin<&mut Probe>) + Send + 'static) -> bool {
            self.0.send(Box::new(call)).is_ok()
        }
    }

    /// Run the ORF finder on a fixture through `run_operation`; returns the
    /// session that came back, the result, and the job list row's status.
    fn orf_run(cancel: bool) -> (String, Session, Result<Finished, String>, &'static str) {
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
        let (calls, queue) = mpsc::channel::<Call>();
        let (tx, rx) = mpsc::channel();
        let label = format!("ORF fixture {cancel}");
        let work = run_operation(
            Window(calls),
            crate::jobs::Listing {
                operation: label.clone(),
                source: file.display().to_string(),
            },
            &mut session,
            move |session| {
                DocumentJob::submit(
                    session,
                    doc,
                    "basecraft.orf-finder/find_orfs@1",
                    serde_json::json!({"min_codons":1,"genetic_code":11}),
                    &[],
                )
            },
            |_, _| {},
            move |_, session, result| tx.send((session, result)).unwrap(),
        )
        .unwrap();
        assert!(session.is_none(), "the job owns the session while it runs");
        if cancel {
            work.cancel();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let (session, result) = loop {
            if let Ok(call) = queue.recv_timeout(std::time::Duration::from_millis(100)) {
                call(std::pin::Pin::new(&mut Probe));
            }
            if let Ok(done) = rx.try_recv() {
                break done;
            }
            assert!(std::time::Instant::now() < deadline);
        };
        let status = crate::jobs::listed()
            .into_iter()
            .find(|e| e.operation_is(&label))
            .unwrap()
            .status;
        drop(temp);
        (identity, session, result, status)
    }

    #[test]
    fn a_plugin_run_returns_the_session_and_finishes_in_the_job_list() {
        let (identity, session, result, status) = orf_run(false);
        assert_eq!(session.id(), identity);
        let finished = result.unwrap();
        assert_eq!(
            finished.outcome.state,
            basecraft_protocol::operation::RuntimeState::Completed,
            "{:?}",
            finished.outcome
        );
        assert_eq!(status, "completed");
    }

    #[test]
    fn a_cancelled_plugin_run_returns_the_session_and_shows_cancelled() {
        let (identity, session, result, status) = orf_run(true);
        assert_eq!(session.id(), identity);
        let cancelled = match result {
            Err(_) => true,
            Ok(finished) => {
                finished.outcome.state == basecraft_protocol::operation::RuntimeState::Cancelled
            }
        };
        assert!(cancelled, "a cancel before the run ends must stop it");
        assert_eq!(status, "cancelled");
    }
}
