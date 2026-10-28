//! Caller-owned path/effect restrictions, shared by all operation adapters.
//! This constrains host actions; trusted plugin code is not OS-sandboxed.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use basecraft_protocol::operation::{InputReference, MAX_INPUT_FILES, OperationRequest};

use crate::{Error, Result, registry::RegisteredOperation};

#[derive(Clone, Debug)]
pub struct SessionPolicy {
    pub root: PathBuf,
    pub document_write: bool,
    pub artifact_write: bool,
}

#[derive(Debug)]
pub struct ResolvedBindings {
    pub inputs: BTreeMap<String, Vec<PathBuf>>,
    /// Session documents named by an input, with their position in that input's
    /// list. The runner snapshots them from the owning session.
    pub documents: BTreeMap<String, Vec<(usize, DocumentBinding)>>,
    pub destinations: BTreeMap<String, PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentBinding {
    pub doc: String,
    pub generation: u64,
    pub revision: u64,
}

fn denied(message: &str) -> Error {
    Error::Protocol(format!("permission_denied: {message}"))
}

impl SessionPolicy {
    pub fn new(root: &Path, writable: bool) -> Result<Self> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(denied("project root must be a directory"));
        }
        Ok(Self {
            root,
            document_write: writable,
            artifact_write: writable,
        })
    }

    pub fn permits(&self, operation: &RegisteredOperation) -> bool {
        operation.package.enabled
            && (!operation.descriptor.effects.document_write || self.document_write)
            && (!operation.descriptor.effects.artifact_write || self.artifact_write)
    }

    pub fn check(&self, operation: &RegisteredOperation) -> Result<()> {
        if !operation.package.enabled {
            return Err(Error::Protocol("plugin_disabled".into()));
        }
        if !self.permits(operation) {
            return Err(denied("operation requires writes; session is read-only"));
        }
        Ok(())
    }

    /// Refuse traversal and symbolic ancestors before canonical resolution. The
    /// runner rechecks destination ownership immediately before publication.
    pub fn path(&self, supplied: &str, output: bool) -> Result<PathBuf> {
        let path = Path::new(supplied);
        if supplied.is_empty() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(denied("path must not be empty or contain parent traversal"));
        }
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        if !path.starts_with(&self.root) {
            return Err(denied("path lies outside session root"));
        }
        let mut current = self.root.clone();
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| denied("path lies outside session root"))?;
        for component in relative.components() {
            current.push(component);
            match current.symlink_metadata() {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(denied("symbolic links are not allowed in bindings"));
                }
                Ok(_) => {}
                Err(e) if output && e.kind() == std::io::ErrorKind::NotFound && current == path => {
                }
                Err(e) => return Err(e.into()),
            }
        }
        if output {
            if path.symlink_metadata().is_ok() {
                return Err(Error::Protocol(
                    "output_conflict: destination already exists".into(),
                ));
            }
            let parent = path
                .parent()
                .ok_or_else(|| denied("missing destination parent"))?
                .canonicalize()?;
            if !parent.starts_with(&self.root) {
                return Err(denied("destination parent lies outside root"));
            }
        } else {
            let metadata = path.metadata()?;
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(denied(
                    "input must be a regular file without hardlink aliases",
                ));
            }
        }
        Ok(path)
    }

    pub fn resolve(
        &self,
        operation: &RegisteredOperation,
        request: &OperationRequest,
    ) -> Result<ResolvedBindings> {
        self.check(operation)?;
        if request.operation != operation.id {
            return Err(Error::Protocol("operation identity mismatch".into()));
        }
        if request
            .inputs
            .keys()
            .any(|k| !operation.descriptor.inputs.contains_key(k))
            || request
                .destinations
                .keys()
                .any(|k| destination_role(operation, k).is_none())
        {
            return Err(Error::Protocol(
                "invalid_arguments: unknown input/destination binding".into(),
            ));
        }
        let mut inputs = BTreeMap::new();
        let mut documents = BTreeMap::new();
        let mut identities = BTreeSet::new();
        let mut count: usize = 0;
        for (name, binding) in &operation.descriptor.inputs {
            let values = request.inputs.get(name).map_or(&[][..], Vec::as_slice);
            count = count.saturating_add(values.len());
            if values.len() < binding.min || values.len() > binding.max || count > MAX_INPUT_FILES {
                return Err(Error::Protocol(format!(
                    "invalid_arguments: input cardinality for {name}"
                )));
            }
            let mut files = Vec::new();
            let mut bound = Vec::new();
            for (position, value) in values.iter().enumerate() {
                let path = match value {
                    InputReference::File { path } => path,
                    InputReference::Document {
                        doc,
                        generation,
                        revision,
                    } => {
                        bound.push((
                            position,
                            DocumentBinding {
                                doc: doc.clone(),
                                generation: *generation,
                                revision: *revision,
                            },
                        ));
                        continue;
                    }
                };
                let path = self.path(path, false)?;
                let metadata = path.metadata()?;
                if !identities.insert((metadata.dev(), metadata.ino())) {
                    return Err(denied("input aliases another input"));
                }
                files.push(path);
            }
            inputs.insert(name.clone(), files);
            if !bound.is_empty() {
                documents.insert(name.clone(), bound);
            }
        }
        let mut destinations = BTreeMap::new();
        let mut unique = BTreeSet::new();
        for (name, path) in &request.destinations {
            let resolved = self.path(path, true)?;
            // Canonical parent prevents lexical aliases such as './'.
            let key = resolved
                .parent()
                .ok_or_else(|| denied("missing parent"))?
                .canonicalize()?
                .join(
                    resolved
                        .file_name()
                        .ok_or_else(|| denied("missing filename"))?,
                );
            if !unique.insert(key) {
                return Err(Error::Protocol(
                    "output_conflict: destinations alias each other".into(),
                ));
            }
            destinations.insert(name.clone(), resolved);
        }
        Ok(ResolvedBindings {
            inputs,
            documents,
            destinations,
        })
    }
}

/// A destination names an output role, or `role/member` to export one member of
/// a collection role as a single file. Returns the role and optional member.
pub fn destination_role<'a>(
    operation: &RegisteredOperation,
    key: &'a str,
) -> Option<(&'a str, Option<&'a Path>)> {
    if operation.descriptor.outputs.contains_key(key) || is_workspace_destination(operation, key) {
        return Some((key, None));
    }
    let (role, member) = key.split_once('/')?;
    let binding = operation.descriptor.outputs.get(role)?;
    let member = Path::new(member);
    (binding.collection
        && !member.as_os_str().is_empty()
        && member
            .components()
            .all(|component| matches!(component, Component::Normal(_))))
    .then_some((role, Some(member)))
}

/// `workspace` asks a species delimitation run to publish its workspace as a
/// SPART file; it is not an output role of the plugin.
pub const WORKSPACE_DESTINATION: &str = "workspace";

pub fn is_workspace_destination(operation: &RegisteredOperation, key: &str) -> bool {
    key == WORKSPACE_DESTINATION
        && matches!(
            operation.package.manifest.id.as_str(),
            "basecraft.delimitation" | "basecraft.host"
        )
}
