//! A species analysis as a job: the run `basecraft_ops::species_analysis`
//! builds, completed with what only the installed operation's descriptor
//! says (the settings it takes, the input it reads the file through, the
//! member that holds a display tree). The window, the CLI and MCP submit the
//! request this returns.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use basecraft_ops::species_analysis::{self, Method, Request};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::operation::{
    InputReference, InputView, OperationDescriptor, OperationErrorCode, OperationRequest,
};

use crate::{Error, Result, registry::Registry};

/// A species analysis ready to submit.
#[derive(Debug)]
pub struct Analysis {
    pub method: Method,
    /// The request, aimed at the workspace, whose history records the run.
    pub request: OperationRequest,
    /// The new file a display tree goes to, beside the workspace.
    pub output: PathBuf,
}

/// The job for `request` on the species workspace `doc`.
pub fn prepare(
    registry: &Registry,
    session: &Session,
    doc: DocId,
    request: &Request,
) -> Result<Analysis> {
    let method = request.method;
    let operation = method.operation();
    let descriptor = &registry
        .operations
        .get(&operation)
        .ok_or_else(|| {
            Error::refused(
                OperationErrorCode::PluginUnavailable,
                format!("{operation} is not installed"),
            )
        })?
        .descriptor;
    let run = species_analysis::run(session, doc, request, &declared(descriptor))?;
    let role = input_role(descriptor, &run.input).ok_or_else(|| {
        Error::refused(
            OperationErrorCode::InvalidArguments,
            format!("{operation} cannot read {}", run.input.display()),
        )
    })?;
    let mut exports = std::collections::BTreeMap::new();
    if method.is_display() {
        let member = primary_member(descriptor).ok_or_else(|| {
            Error::refused(
                OperationErrorCode::PluginProtocolViolation,
                format!("{operation} declares no primary result"),
            )
        })?;
        exports.insert(member, run.output.clone());
    }
    let root = session.root().canonicalize()?;
    let inputs = [(role, vec![run.input])].into();
    let mut job = basecraft_ops::requests::file_request(
        &root,
        &operation,
        &inputs,
        run.parameters,
        &exports,
    )?;
    job.target = Some(InputReference::Document {
        doc: doc.0.to_string(),
        generation: session.generation(doc)?,
        revision: session.revision(doc)?,
    });
    Ok(Analysis {
        method,
        request: job,
        output: run.output,
    })
}

/// The parameters an operation's descriptor declares; others are refused.
pub fn declared(descriptor: &OperationDescriptor) -> BTreeSet<String> {
    descriptor
        .parameters
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

/// The input an operation reads `path` through: its only input, or the one
/// whose view matches a tree or an alignment file.
pub fn input_role(descriptor: &OperationDescriptor, path: &Path) -> Option<String> {
    let inputs = &descriptor.inputs;
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
}

/// The collection member that holds an operation's main result, such as
/// `tree/tree.nwk`, as its descriptor declares it.
pub fn primary_member(descriptor: &OperationDescriptor) -> Option<String> {
    descriptor
        .outputs
        .iter()
        .find_map(|(role, output)| Some(format!("{role}/{}", output.primary.as_ref()?)))
}
