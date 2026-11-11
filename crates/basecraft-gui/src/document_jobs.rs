//! Registered operations run on an open document. The run happens on a job
//! thread that owns the document's Session, so the session records the run in
//! the document's history before the window hears of its completion.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
                generation: session.generation(target).map_err(|e| e.to_string())?,
                revision: session.revision(target).map_err(|e| e.to_string())?,
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
    session: &mut Option<Session>,
    prepare: impl FnOnce(&mut Session) -> Result<DocumentJob, String> + Send + 'static,
    progress: fn(std::pin::Pin<&mut T>, &serde_json::Value),
    done: impl FnOnce(std::pin::Pin<&mut T>, Session, Result<Finished, String>) + Send + 'static,
) -> Result<basecraft_ops::work::Work, String> {
    let reporter = owner.clone();
    crate::jobs::with_session(
        owner,
        true,
        session,
        move |session, work| {
            if work.cancelled() {
                return Err("Operation cancelled before preparation".into());
            }
            let job = prepare(session)?;
            let file = job.progress_file();
            let (mut shown, mut cancelling, mut tick) = (None, false, 0u8);
            loop {
                tick = tick.wrapping_add(1);
                if work.cancelled() && !cancelling {
                    job.cancel()?;
                    cancelling = true;
                }
                if let Some(outcome) = job.poll(session)? {
                    return Ok(Finished {
                        outcome,
                        generation: job.finalized_generation(),
                    });
                }
                // The plugin rewrites its progress file a few times a second.
                if tick % 8 == 0
                    && let Some(value) = file.as_deref().and_then(read_progress)
                    && shown.as_ref() != Some(&value)
                {
                    shown = Some(value.clone());
                    work.progress("");
                    reporter.deliver(move |owner| progress(owner, &value));
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        },
        done,
    )
}
