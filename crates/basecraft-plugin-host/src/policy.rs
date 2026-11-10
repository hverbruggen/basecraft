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

    /// Unwired strict backend for future contextual owners. This is a resource
    /// check, not service eligibility, overwrite consent or a publication grant.
    /// Keep the returned observation alive; its path is not permission to reopen.
    #[allow(dead_code)]
    pub(crate) fn resolve_resource_strict(
        &self,
        supplied: &Path,
        action: ResourceAction,
    ) -> std::result::Result<StrictResource, StrictResourceError> {
        self.resolve_resource_strict_with(
            supplied,
            action,
            basecraft_platform::files::namespace_lock_supported(),
            basecraft_platform::files::inspect_target_namespace,
        )
    }

    // The only strict IO boundary is the actual platform inspector. No legacy
    // callbacks are present here, including link_count's content-capable open.
    fn resolve_resource_strict_with(
        &self,
        supplied: &Path,
        action: ResourceAction,
        supported: bool,
        inspect: impl FnOnce(
            &Path,
        ) -> std::result::Result<
            basecraft_platform::files::TargetNamespace,
            basecraft_platform::files::NamespaceReadError,
        >,
    ) -> std::result::Result<StrictResource, StrictResourceError> {
        self.resource_effects(action)?;
        let (path, relative) = self.resource_path(supplied, action)?;
        if !supported {
            return Err(AccessError::Denied(Denial::ServiceUnavailable).into());
        }
        // PathBuf::join("") retains a trailing separator. The legacy resolver
        // accepts that spelling, but the strict platform rejects raw trailing
        // separators. Use the already configured root spelling, without IO.
        let path = if relative.as_os_str().is_empty() {
            self.root.clone()
        } else {
            path
        };
        let namespace = inspect(&path).map_err(strict_namespace_error)?;
        let metadata = namespace.metadata();
        if relative.as_os_str().is_empty() {
            // The configured resource root must itself still be a directory.
            // For a descendant, the shared platform chain establishes this.
            if let Some(info) = metadata {
                resource_directory(
                    false,
                    info.kind() == basecraft_platform::files::OrdinaryKind::Directory,
                )?;
            } else {
                return Err(StrictResourceError::Missing(Box::new(namespace)));
            }
        }
        if metadata.is_none() && !resource_allows_absence(action) {
            // S2a exposes this error by borrow. Retain its owner instead of
            // manufacturing a new NotFound or losing its raw OS/source value.
            return Err(StrictResourceError::Missing(Box::new(namespace)));
        }
        resource_leaf(
            action,
            metadata.map(|info| {
                (
                    info.kind() == basecraft_platform::files::OrdinaryKind::File,
                    info.kind() == basecraft_platform::files::OrdinaryKind::Directory,
                )
            }),
            || Ok(metadata.map_or(0, basecraft_platform::files::TargetMetadata::links)),
        )?;
        Ok(StrictResource { path, namespace })
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

/// Retained structural observation, never a service/consent token. No production
/// consumer yet: S2c must bind its own request and reauthorize actual effects.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct StrictResource {
    path: PathBuf,
    namespace: basecraft_platform::files::TargetNamespace,
}

/// Local resolver failure carrier, not a new public Access error category.
/// Missing retains S2a's actual checked-absence owner and original IO error.
#[derive(Debug)]
pub(crate) enum StrictResourceError {
    Access(AccessError),
    Missing(Box<basecraft_platform::files::TargetNamespace>),
}
#[allow(dead_code)] // Contextual host adapter remains deliberately unwired.
impl StrictResourceError {
    fn into_access(self) -> AccessError {
        match self {
            Self::Access(error) => error,
            Self::Missing(namespace) => match namespace.into_absence_error() {
                Ok(error) => AccessError::Io(error),
                Err(_) => AccessError::Denied(Denial::ServiceUnavailable),
            },
        }
    }
}
impl From<AccessError> for StrictResourceError {
    fn from(error: AccessError) -> Self {
        Self::Access(error)
    }
}
impl std::fmt::Display for StrictResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Access(error) => error.fmt(f),
            Self::Missing(namespace) => match namespace.absence_error() {
                Some(error) => error.fmt(f),
                None => f.write_str("checked missing resource has no absence observation"),
            },
        }
    }
}
impl std::error::Error for StrictResourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Access(error) => Some(error),
            Self::Missing(namespace) => namespace
                .absence_error()
                .map(|error| error as &(dyn std::error::Error + 'static)),
        }
    }
}
fn strict_namespace_error(error: basecraft_platform::files::NamespaceReadError) -> AccessError {
    use basecraft_platform::files::NamespaceReadError;
    match error {
        NamespaceReadError::Unsupported | NamespaceReadError::Changed => {
            AccessError::Denied(Denial::ServiceUnavailable)
        }
        NamespaceReadError::Redirected => AccessError::Denied(Denial::SymbolicLink),
        NamespaceReadError::Aliased => AccessError::Denied(Denial::Hardlink),
        NamespaceReadError::WrongKind => AccessError::Denied(Denial::ResourceType),
        NamespaceReadError::Io(error) => AccessError::Io(error),
    }
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

// Deliberately private and unwired. Runtime construction needs separate review.
#[allow(dead_code)]
struct SessionServiceAccess<'a> {
    policy: &'a SessionPolicy,
    context: &'a basecraft_ops::service_reads::ServiceReadContext,
}

#[allow(dead_code)]
type SessionServiceReadAccess<'a> = SessionServiceAccess<'a>;

#[allow(dead_code)] // No production construction until separately reviewed activation.
impl SessionServiceAccess<'_> {
    fn lock_policy(
        &self,
        request: &basecraft_ops::service_reads::ServiceRequest<'_>,
        phase: &basecraft_ops::service_reads::ServicePhase<'_>,
    ) -> basecraft_ops::access::Result<()> {
        self.policy.resource_effects(ResourceAction::ReplaceFile)?;
        if !request.permits_lock(self.context, phase) {
            return Err(AccessError::Denied(Denial::ServiceUnavailable));
        }
        use basecraft_ops::service_reads::ServicePhase;
        let target = match phase {
            ServicePhase::Eligibility => request.target(),
            ServicePhase::LockValidated(ticket)
            | ServicePhase::BeforeEffect(ticket, _)
            | ServicePhase::Held(ticket) => ticket.target(),
            ServicePhase::Validated(_) => None, // rejected by permits_lock above
        };
        if let Some((target, action)) = target {
            let (path, _) = self.policy.resource_path(target, action)?;
            let parent = path
                .parent()
                .ok_or(AccessError::Denied(Denial::InvalidPath))?;
            self.policy
                .resource_path(parent, ResourceAction::ReadDirectory)?;
        }
        Ok(())
    }
}

impl Access for SessionServiceAccess<'_> {
    fn boundary(&self) -> Option<&Path> {
        self.policy.boundary()
    }
    fn resource(
        &self,
        path: &Path,
        action: ResourceAction,
    ) -> basecraft_ops::access::Result<PathBuf> {
        self.policy.resource(path, action)
    }
    fn service_context(
        &self,
        request: &basecraft_ops::service_reads::ServiceRequest<'_>,
        phase: basecraft_ops::service_reads::ServicePhase<'_>,
    ) -> basecraft_ops::access::Result<()> {
        self.lock_policy(request, &phase)
    }
    fn service_target<'a>(
        &self,
        request: &'a basecraft_ops::service_reads::ServiceRequest<'a>,
    ) -> basecraft_ops::access::Result<basecraft_ops::service_reads::CheckedServiceTarget<'a>> {
        self.lock_policy(
            request,
            &basecraft_ops::service_reads::ServicePhase::Eligibility,
        )?;
        let (target, action) = request
            .target()
            .ok_or(AccessError::Denied(Denial::ServiceUnavailable))?;
        let resolved = self
            .policy
            .resolve_resource_strict(target, action)
            .map_err(StrictResourceError::into_access)?;
        request.checked_target(resolved.path, resolved.namespace)
    }
    fn service_read(
        &self,
        request: &basecraft_ops::service_reads::ServiceReadRequest<'_>,
        phase: basecraft_ops::service_reads::ServiceReadPhase<'_>,
    ) -> basecraft_ops::access::Result<()> {
        if request.permits(self.context, &phase) {
            Ok(())
        } else {
            Err(AccessError::Denied(Denial::ServiceUnavailable))
        }
    }
}

#[cfg(test)]
mod service_read_tests {
    use super::*;
    use basecraft_ops::access::{ServiceAction, TrustedLegacy};
    use basecraft_ops::service_reads::{ServiceReadContext, ServiceReadPhase, ServiceReadRequest};
    use std::cell::Cell;

    #[test]
    fn only_explicit_bound_readonly_adapter_allows_eligibility() {
        struct Inspect<'a> {
            adapter: &'a SessionServiceReadAccess<'a>,
            other: &'a ServiceReadContext,
            called: Cell<bool>,
        }
        impl Access for Inspect<'_> {
            fn boundary(&self) -> Option<&Path> {
                self.adapter.boundary()
            }
            fn resource(
                &self,
                _: &Path,
                _: ResourceAction,
            ) -> basecraft_ops::access::Result<PathBuf> {
                panic!()
            }
            fn service_read(
                &self,
                request: &ServiceReadRequest<'_>,
                phase: ServiceReadPhase<'_>,
            ) -> basecraft_ops::access::Result<()> {
                assert!(matches!(phase, ServiceReadPhase::Eligibility));
                assert!(self.adapter.service_read(request, phase).is_ok());
                assert!(
                    self.adapter
                        .policy
                        .service_read(request, ServiceReadPhase::Eligibility)
                        .is_err()
                );
                assert!(
                    TrustedLegacy
                        .service_read(request, ServiceReadPhase::Eligibility)
                        .is_err()
                );
                assert!(
                    SessionServiceReadAccess {
                        policy: self.adapter.policy,
                        context: self.other
                    }
                    .service_read(request, ServiceReadPhase::Eligibility)
                    .is_err()
                );
                self.called.set(true);
                // Stop before any real process configuration is probed.
                Err(AccessError::Denied(Denial::ServiceUnavailable))
            }
        }
        let root = tempfile::tempdir().unwrap();
        let policy = SessionPolicy::new(root.path(), false).unwrap();
        let context = ServiceReadContext::from_process().unwrap();
        let other = ServiceReadContext::from_process().unwrap();
        let adapter = SessionServiceReadAccess {
            policy: &policy,
            context: &context,
        };
        let inspect = Inspect {
            adapter: &adapter,
            other: &other,
            called: Cell::new(false),
        };
        assert!(context.preferences(&inspect).is_err());
        assert_eq!(
            inspect.called.get(),
            basecraft_platform::files::namespace_read_supported()
        );
        for action in [
            ServiceAction::ReadPreferences,
            ServiceAction::ReadAnalysisRegistry,
            ServiceAction::LockAnalysisEvidence,
            ServiceAction::ReserveSaveTarget,
            ServiceAction::RegisterFinalStore,
        ] {
            assert!(adapter.service(action).is_err());
        }
        // Service binding neither grants outside resource reads nor write effects.
        let outside = root.path().parent().unwrap().join("outside-not-probed");
        assert!(matches!(
            adapter.resource(&outside, ResourceAction::Probe),
            Err(AccessError::Denied(Denial::OutsideRoot))
        ));
        assert!(matches!(
            adapter.resource(&root.path().join("new"), ResourceAction::CreateFile),
            Err(AccessError::Denied(Denial::ReadOnly))
        ));
        assert_eq!(adapter.boundary(), policy.boundary());
    }
    #[test]
    fn lock_adapter_requires_both_flags_and_exact_context_before_bootstrap() {
        use basecraft_ops::service_reads::{
            SaveTargetPurpose, ServiceContext, ServicePhase, ServiceRequest,
        };
        struct Gate<'a> {
            adapter: SessionServiceAccess<'a>,
            expected: bool,
            called: Cell<usize>,
            other: &'a ServiceContext,
        }
        impl Access for Gate<'_> {
            fn boundary(&self) -> Option<&Path> {
                self.adapter.boundary()
            }
            fn resource(
                &self,
                _: &Path,
                _: ResourceAction,
            ) -> basecraft_ops::access::Result<PathBuf> {
                panic!("no legacy resolution")
            }
            fn service_context(
                &self,
                request: &ServiceRequest<'_>,
                phase: ServicePhase<'_>,
            ) -> basecraft_ops::access::Result<()> {
                assert!(matches!(phase, ServicePhase::Eligibility));
                self.called.set(self.called.get() + 1);
                assert_eq!(
                    self.adapter.service_context(request, phase).is_ok(),
                    self.expected && basecraft_platform::files::namespace_lock_supported()
                );
                assert!(
                    self.adapter
                        .policy
                        .service_context(request, ServicePhase::Eligibility)
                        .is_err()
                );
                assert!(
                    TrustedLegacy
                        .service_context(request, ServicePhase::Eligibility)
                        .is_err()
                );
                assert!(
                    self.adapter
                        .service_read(request, ServicePhase::Eligibility)
                        .is_err()
                );
                assert!(
                    SessionServiceAccess {
                        policy: self.adapter.policy,
                        context: self.other
                    }
                    .service_context(request, ServicePhase::Eligibility)
                    .is_err()
                );
                Err(AccessError::Denied(Denial::ServiceUnavailable)) // before target or lock namespace inspection
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let context = std::sync::Arc::new(ServiceContext::from_process().unwrap());
        let other = ServiceContext::from_process().unwrap();
        for (document, artifact) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut policy = SessionPolicy::new(dir.path(), true).unwrap();
            policy.document_write = document;
            policy.artifact_write = artifact;
            let gate = Gate {
                adapter: SessionServiceAccess {
                    policy: &policy,
                    context: &context,
                },
                expected: document && artifact,
                called: Cell::new(0),
                other: &other,
            };
            assert!(context.lock_evidence(&gate).is_err());
            assert!(
                context
                    .reserve_target(&gate, &dir.path().join("new"), SaveTargetPurpose::NewFile)
                    .is_err()
            );
            assert_eq!(
                gate.called.get(),
                2 * usize::from(basecraft_platform::files::namespace_lock_supported())
            );
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn lock_target_handoff_preserves_metadata_and_owned_missing_error() {
        use basecraft_ops::service_reads::{
            CheckedServiceTarget, SaveTargetPurpose, ServiceContext, ServicePhase, ServiceRequest,
        };
        use std::os::unix::fs::PermissionsExt;
        struct TargetGate<'a> {
            adapter: SessionServiceAccess<'a>,
            calls: Cell<usize>,
            expected_present: bool,
        }
        impl Access for TargetGate<'_> {
            fn boundary(&self) -> Option<&Path> {
                self.adapter.boundary()
            }
            fn resource(
                &self,
                _: &Path,
                _: ResourceAction,
            ) -> basecraft_ops::access::Result<PathBuf> {
                panic!("no legacy resolver")
            }
            fn service_context(
                &self,
                request: &ServiceRequest<'_>,
                phase: ServicePhase<'_>,
            ) -> basecraft_ops::access::Result<()> {
                assert!(matches!(phase, ServicePhase::Eligibility));
                self.adapter.service_context(request, phase)
            }
            fn service_target<'a>(
                &self,
                request: &'a ServiceRequest<'a>,
            ) -> basecraft_ops::access::Result<CheckedServiceTarget<'a>> {
                self.calls.set(self.calls.get() + 1);
                let result = self.adapter.service_target(request);
                if self.expected_present {
                    assert!(result.is_ok());
                    // Stop after the actual strict-result handoff, before lock
                    // namespace inspection or any service artifact creation.
                    Err(AccessError::Denied(Denial::ServiceUnavailable))
                } else {
                    result
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unreadable");
        std::fs::write(&path, "preserve").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
        let policy = SessionPolicy::new(dir.path(), true).unwrap();
        let context = std::sync::Arc::new(ServiceContext::from_process().unwrap());
        let gate = TargetGate {
            adapter: SessionServiceAccess {
                policy: &policy,
                context: &context,
            },
            calls: Cell::new(0),
            expected_present: true,
        };
        assert!(
            context
                .reserve_target(&gate, &path, SaveTargetPurpose::ReplaceFile)
                .is_err()
        );
        assert_eq!(gate.calls.get(), 1);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let gate = TargetGate {
            expected_present: false,
            ..gate
        };
        let result = context.reserve_target(
            &gate,
            &dir.path().join("missing"),
            SaveTargetPurpose::ReplaceFile,
        );
        assert!(
            matches!(result,Err(basecraft_ops::Error::Access(AccessError::Io(error))) if error.kind()==std::io::ErrorKind::NotFound && error.raw_os_error().is_some())
        );
        assert_eq!(gate.calls.get(), 2);
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn effect_policy_checks_retained_absolute_target_after_root_rebinding() {
        use basecraft_ops::service_reads::{
            CheckedServiceTarget, SaveTargetPurpose, ServiceContext, ServicePhase, ServiceRequest,
        };
        struct Rebound<'a> {
            initial: SessionServiceAccess<'a>,
            later: SessionServiceAccess<'a>,
            validated: Cell<bool>,
        }
        impl Access for Rebound<'_> {
            fn boundary(&self) -> Option<&Path> {
                self.initial.boundary()
            }
            fn resource(
                &self,
                _: &Path,
                _: ResourceAction,
            ) -> basecraft_ops::access::Result<PathBuf> {
                panic!("legacy resolver")
            }
            fn service_context(
                &self,
                request: &ServiceRequest<'_>,
                phase: ServicePhase<'_>,
            ) -> basecraft_ops::access::Result<()> {
                match phase {
                    ServicePhase::Eligibility => self.initial.service_context(request, phase),
                    ServicePhase::LockValidated(_) => {
                        self.validated.set(true);
                        self.later.service_context(request, phase)
                    }
                    _ => panic!("no effect after root rebind refusal"),
                }
            }
            fn service_target<'a>(
                &self,
                request: &'a ServiceRequest<'a>,
            ) -> basecraft_ops::access::Result<CheckedServiceTarget<'a>> {
                self.initial.service_target(request)
            }
        }
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let initial = SessionPolicy::new(first.path(), true).unwrap();
        let later = SessionPolicy::new(second.path(), true).unwrap();
        let context = std::sync::Arc::new(ServiceContext::from_process().unwrap());
        let access = Rebound {
            initial: SessionServiceAccess {
                policy: &initial,
                context: &context,
            },
            later: SessionServiceAccess {
                policy: &later,
                context: &context,
            },
            validated: Cell::new(false),
        };
        assert!(matches!(
            context.reserve_target(&access, Path::new("new"), SaveTargetPurpose::NewFile),
            Err(basecraft_ops::Error::Access(AccessError::Denied(
                Denial::OutsideRoot
            )))
        ));
        assert!(access.validated.get());
        assert!(!first.path().join("new").exists());
        assert!(!second.path().join("new").exists());
    }
}
