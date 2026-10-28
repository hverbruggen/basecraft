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
    pub started: Instant,
    root: PathBuf,
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
        let mut manager = JobManager::default();
        let accepted = manager
            .submit_in_session(&registry, request, policy, "GUI", session)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            manager,
            run_id: accepted.run_id,
            started: Instant::now(),
            root,
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
        let mut manager = JobManager::default();
        let accepted = manager
            .submit_in_session(&registry, request, policy, "GUI", session)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            manager,
            run_id: accepted.run_id,
            started: Instant::now(),
            root,
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
