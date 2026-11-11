//! Caller-owned path/effect restrictions, shared by all operation adapters.
//! This constrains host actions; trusted plugin code is not OS-sandboxed.

use basecraft_ops::access::{Access, AccessError, Denial, ResourceAction};
use basecraft_platform::files::{identity, link_count};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use basecraft_protocol::operation::{InputReference, MAX_INPUT_FILES, OperationRequest};

use crate::{Error, Result, registry::RegisteredOperation};

/// Runtime-only process payload. Never serialized or printed by Debug.
#[derive(Clone, Default)]
pub(crate) struct RuntimeCredentials(pub(crate) BTreeMap<String, String>);
impl std::fmt::Debug for RuntimeCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RuntimeCredentials([redacted])")
    }
}
impl RuntimeCredentials {
    pub(crate) fn redact(&self, text: &str) -> String {
        let mut values: Vec<_> = self.0.values().filter(|value| !value.is_empty()).collect();
        values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        values.into_iter().fold(text.to_owned(), |text, value| {
            text.replace(value.as_str(), "[redacted]")
        })
    }
    pub(crate) fn redact_json(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => *text = self.redact(text),
            serde_json::Value::Array(values) => {
                for value in values {
                    self.redact_json(value);
                }
            }
            serde_json::Value::Object(values) => {
                *values = std::mem::take(values)
                    .into_iter()
                    .map(|(key, mut value)| {
                        self.redact_json(&mut value);
                        (self.redact(&key), value)
                    })
                    .collect();
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
struct CredentialGrant {
    operation: String,
    package_digest: String,
    descriptor_digest: String,
    name: String,
    value: String,
}
impl std::fmt::Debug for CredentialGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialGrant")
            .field("operation", &self.operation)
            .field("value", &"[redacted]")
            .finish_non_exhaustive()
    }
}

pub(crate) fn credential_environment(name: &str) -> bool {
    name != "BASECRAFT_TOKEN"
        && name.starts_with("BASECRAFT_")
        && name.ends_with("_TOKEN")
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Clone, Debug)]
pub struct SessionPolicy {
    pub root: PathBuf,
    pub document_write: bool,
    pub artifact_write: bool,
    credential_grants: Vec<CredentialGrant>,
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
            credential_grants: Vec::new(),
        })
    }

    /// Grant a declared logical credential to this exact resolved operation and
    /// package/descriptor digest. A declaration alone never grants ambient access.
    pub fn with_runtime_credential(
        mut self,
        operation: &RegisteredOperation,
        name: &str,
        value: String,
    ) -> Result<Self> {
        operation.package.verify()?;
        if operation
            .descriptor
            .credentials
            .get(name)
            .is_none_or(|environment| !credential_environment(environment))
            || value.is_empty()
            || value.len() > 8192
            || value.contains(['\0', '\r', '\n'])
        {
            return Err(denied("invalid runtime credential grant"));
        }
        self.credential_grants.push(CredentialGrant {
            operation: operation.id.clone(),
            package_digest: operation.package.digest.clone(),
            descriptor_digest: operation.descriptor_digest.clone(),
            name: name.into(),
            value,
        });
        Ok(self)
    }

    /// Compatibility provider for the bundled Zenodo operation, not an arbitrary
    /// package claiming its ID or declaring the same environment variable.
    pub(crate) fn capture_ambient_credential(
        self,
        operation: &RegisteredOperation,
    ) -> Result<Self> {
        if operation.package.origin == crate::registry::Origin::Bundled
            && operation.id == "basecraft.zenodo/draft@1"
            && operation
                .descriptor
                .credentials
                .get("zenodo_access")
                .is_some_and(|name| name == "BASECRAFT_ZENODO_TOKEN")
            && !self
                .credential_grants
                .iter()
                .any(|grant| grant.matches(operation, "zenodo_access"))
            && let Ok(value) = std::env::var("BASECRAFT_ZENODO_TOKEN")
            && !value.is_empty()
        {
            return self.with_runtime_credential(operation, "zenodo_access", value);
        }
        Ok(self)
    }

    pub(crate) fn credentials_for(&self, operation: &RegisteredOperation) -> RuntimeCredentials {
        RuntimeCredentials(
            operation
                .descriptor
                .credentials
                .iter()
                .filter_map(|(name, environment)| {
                    self.credential_grants
                        .iter()
                        .rev()
                        .find(|grant| grant.matches(operation, name))
                        .map(|grant| (environment.clone(), grant.value.clone()))
                })
                .collect(),
        )
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
        let action = if output {
            ResourceAction::CreateFile
        } else {
            ResourceAction::ReadFile
        };
        // Existing operation dispatch owns effects for this legacy entry point.
        // Both entry points share the same path resolver, with typed foundation
        // errors converted only at this existing protocol boundary.
        self.resolve_resource(Path::new(supplied), action)
            .map_err(|error| match error {
                AccessError::Denied(reason) => denied(&reason.to_string()),
                AccessError::DestinationExists => {
                    Error::Protocol("output_conflict: destination already exists".into())
                }
                AccessError::Io(error) => error.into(),
            })
    }

    fn resolve_resource(
        &self,
        supplied: &Path,
        action: ResourceAction,
    ) -> basecraft_ops::access::Result<PathBuf> {
        self.resolve_resource_with(
            supplied,
            action,
            &mut |path| path.symlink_metadata(),
            &mut link_count,
        )
    }

    fn resource_effects(&self, action: ResourceAction) -> basecraft_ops::access::Result<()> {
        // Recovery mutation may change both documents and evidence artifacts.
        // Do not infer permission from the selected root, handle or receipt.
        if action.writes() && (!self.document_write || !self.artifact_write) {
            return Err(AccessError::Denied(Denial::ReadOnly));
        }
        Ok(())
    }

    fn resource_path(
        &self,
        supplied: &Path,
        action: ResourceAction,
    ) -> basecraft_ops::access::Result<(PathBuf, PathBuf)> {
        let reject = |reason| Err(AccessError::Denied(reason));
        if supplied.as_os_str().is_empty()
            || supplied
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        {
            return reject(Denial::InvalidPath);
        }
        let path = if supplied.is_absolute() {
            supplied.to_path_buf()
        } else {
            self.root.join(supplied)
        };
        let relative: PathBuf = path
            .strip_prefix(&self.root)
            .map_err(|_| AccessError::Denied(Denial::OutsideRoot))?
            .components()
            .filter(|component| !matches!(component, Component::CurDir))
            .collect();
        let path = self.root.join(&relative);
        if relative.as_os_str().is_empty()
            && matches!(
                action,
                ResourceAction::ReplaceFile
                    | ResourceAction::RenameSource
                    | ResourceAction::Cleanup
            )
        {
            return reject(Denial::RootMutation);
        }
        Ok((path, relative))
    }

    // Explicit legacy observation backend. Keep its probe order, original IO,
    // missing-leaf rules and create-parent canonical check for existing callers.
    fn resolve_resource_with(
        &self,
        supplied: &Path,
        action: ResourceAction,
        metadata: &mut impl FnMut(&Path) -> std::io::Result<std::fs::Metadata>,
        links: &mut impl FnMut(&Path) -> std::io::Result<u64>,
    ) -> basecraft_ops::access::Result<PathBuf> {
        let (path, relative) = self.resource_path(supplied, action)?;
        let root_metadata = metadata(&self.root)?;
        resource_directory(
            root_metadata.file_type().is_symlink(),
            root_metadata.is_dir(),
        )?;
        let mut leaf = Some(root_metadata);
        let mut current = self.root.clone();
        for component in relative.components() {
            if matches!(component, Component::CurDir) {
                continue;
            }
            current.push(component);
            match metadata(&current) {
                Ok(info) => {
                    if info.file_type().is_symlink() {
                        return Err(AccessError::Denied(Denial::SymbolicLink));
                    }
                    if current != path {
                        resource_directory(info.file_type().is_symlink(), info.is_dir())?;
                    }
                    leaf = Some(info);
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        && current == path
                        && resource_allows_absence(action) =>
                {
                    leaf = None;
                }
                Err(error) => return Err(error.into()),
            }
        }
        resource_leaf(
            action,
            leaf.as_ref().map(|info| (info.is_file(), info.is_dir())),
            || links(&path),
        )?;
        if action.creates() {
            // Legacy output containment check. Strict observations never call it.
            let parent = path
                .parent()
                .ok_or(AccessError::Denied(Denial::InvalidPath))?
                .canonicalize()?;
            if !parent.starts_with(&self.root) {
                return Err(AccessError::Denied(Denial::OutsideRoot));
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
                if !identities.insert(identity(&path)?) {
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

impl CredentialGrant {
    fn matches(&self, operation: &RegisteredOperation, name: &str) -> bool {
        self.operation == operation.id
            && self.package_digest == operation.package.digest
            && self.descriptor_digest == operation.descriptor_digest
            && self.name == name
    }
}

fn resource_allows_absence(action: ResourceAction) -> bool {
    action.creates() || action == ResourceAction::Probe
}

fn resource_directory(symbolic: bool, directory: bool) -> basecraft_ops::access::Result<()> {
    if symbolic {
        Err(AccessError::Denied(Denial::SymbolicLink))
    } else if !directory {
        Err(AccessError::Denied(Denial::ResourceType))
    } else {
        Ok(())
    }
}

// Shared action/type/no-overwrite decisions. The backend supplies only observed
// kind and deferred link count; the strict backend's count is retained metadata.
fn resource_leaf(
    action: ResourceAction,
    kind: Option<(bool, bool)>,
    links: impl FnOnce() -> std::io::Result<u64>,
) -> basecraft_ops::access::Result<()> {
    if action.creates() {
        if kind.is_some() {
            return Err(AccessError::DestinationExists);
        }
    } else if let Some((file, directory)) = kind {
        if !file && !directory {
            return Err(AccessError::Denied(Denial::ResourceType));
        }
        if matches!(
            action,
            ResourceAction::ReadFile | ResourceAction::ReplaceFile | ResourceAction::SyncFile
        ) && !file
            || matches!(
                action,
                ResourceAction::ReadDirectory | ResourceAction::SyncDirectory
            ) && !directory
        {
            return Err(AccessError::Denied(Denial::ResourceType));
        }
        if file && links()? != 1 {
            return Err(AccessError::Denied(Denial::Hardlink));
        }
    }
    Ok(())
}


impl Access for SessionPolicy {
    fn boundary(&self) -> Option<&Path> {
        Some(&self.root)
    }

    fn resource(
        &self,
        path: &Path,
        action: ResourceAction,
    ) -> basecraft_ops::access::Result<PathBuf> {
        self.resource_effects(action)?;
        self.resolve_resource(path, action)
    }
}

#[cfg(test)]
#[path = "policy_access_tests.rs"]
mod access_tests;
