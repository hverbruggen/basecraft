//! Finite application-owned reads. Unwired: a context is configuration, not permission.
use std::path::{Path, PathBuf};

use crate::access::{Access, AccessError, Denial, ServiceAction};
use basecraft_platform::{
    files::{
        CheckedRead, NamespaceReadError, ReadNamespace, inspect_read_namespace,
        namespace_read_supported,
    },
    paths::AppPaths,
    settings::{Preferences, SettingsStore},
};

pub struct ServiceReadContext {
    preferences: PathBuf,
    registry: PathBuf,
}

pub struct ServiceReadRequest<'a> {
    context: &'a ServiceReadContext,
    action: ServiceAction,
}

/// Borrows the exact request, so a ticket cannot outlive or move its request.
pub struct CheckedServiceNamespace<'a> {
    request: &'a ServiceReadRequest<'a>,
    namespace: ReadNamespace,
}

pub enum ServiceReadPhase<'a> {
    Eligibility,
    Validated(&'a CheckedServiceNamespace<'a>),
}

impl ServiceReadRequest<'_> {
    /// Pure policy predicate; no path resolution, operation verification or IO.
    pub fn permits(&self, context: &ServiceReadContext, phase: &ServiceReadPhase<'_>) -> bool {
        std::ptr::eq(self.context, context)
            && matches!(
                self.action,
                ServiceAction::ReadPreferences | ServiceAction::ReadAnalysisRegistry
            )
            && namespace_read_supported()
            && match phase {
                ServiceReadPhase::Eligibility => true,
                ServiceReadPhase::Validated(ticket) => std::ptr::eq(self, ticket.request),
            }
    }

    fn location(&self) -> crate::Result<&Path> {
        match self.action {
            ServiceAction::ReadPreferences => Ok(&self.context.preferences),
            ServiceAction::ReadAnalysisRegistry => Ok(&self.context.registry),
            _ => Err(AccessError::Denied(Denial::ServiceUnavailable).into()),
        }
    }

    fn read(&self, access: &dyn Access) -> crate::Result<CheckedRead> {
        self.read_with(access, inspect_read_namespace, ReadNamespace::open_read)
    }

    // Seams surround the actual platform boundaries, never a second reader.
    fn read_with(
        &self,
        access: &dyn Access,
        inspect: impl FnOnce(&Path) -> Result<ReadNamespace, NamespaceReadError>,
        consume: impl FnOnce(ReadNamespace) -> Result<CheckedRead, NamespaceReadError>,
    ) -> crate::Result<CheckedRead> {
        if !self.permits(self.context, &ServiceReadPhase::Eligibility) {
            return Err(AccessError::Denied(Denial::ServiceUnavailable).into());
        }
        let location = self.location()?;
        access.service_read(self, ServiceReadPhase::Eligibility)?;
        let ticket = CheckedServiceNamespace {
            request: self,
            namespace: inspect(location).map_err(namespace_error)?,
        };
        access.service_read(self, ServiceReadPhase::Validated(&ticket))?;
        Ok(consume(ticket.namespace).map_err(namespace_error)?)
    }
}

impl ServiceReadContext {
    /// Capture existing process configuration once. Discovery performs no IO.
    pub fn from_process() -> crate::Result<Self> {
        let paths = AppPaths::discover().map_err(|e| crate::error::invalid(e.to_string()))?;
        Self::from_paths(&paths)
    }

    fn from_paths(paths: &AppPaths) -> crate::Result<Self> {
        let context = Self {
            preferences: paths.preferences(),
            registry: crate::input_evidence::registry_location(paths),
        };
        for path in [&context.preferences, &context.registry] {
            // Mirror the supported lexical shape without probing/canonicalizing.
            if !path.is_absolute()
                || path.components().any(|c| {
                    !matches!(
                        c,
                        std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                            | std::path::Component::Normal(_)
                    )
                })
            {
                return Err(AccessError::Denied(Denial::InvalidPath).into());
            }
        }
        Ok(context)
    }

    pub fn preferences(&self, access: &dyn Access) -> crate::Result<Preferences> {
        let input = ServiceReadRequest {
            context: self,
            action: ServiceAction::ReadPreferences,
        }
        .read(access)?;
        SettingsStore::load_checked(input).map_err(|error| match error {
            basecraft_platform::settings::Error::Io(error) => crate::Error::Io(error),
            basecraft_platform::settings::Error::Save(error) => crate::Error::Format(error),
            basecraft_platform::settings::Error::Busy => crate::error::invalid(
                "preferences are being changed by another process; retry shortly",
            ),
            basecraft_platform::settings::Error::Invalid(message) => crate::error::invalid(message),
        })
    }

    pub(crate) fn registry_location(&self) -> &Path {
        &self.registry
    }

    pub(crate) fn registry_input(&self, access: &dyn Access) -> crate::Result<CheckedRead> {
        ServiceReadRequest {
            context: self,
            action: ServiceAction::ReadAnalysisRegistry,
        }
        .read(access)
    }
}

fn namespace_error(error: NamespaceReadError) -> AccessError {
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

#[cfg(test)]
#[path = "service_reads_tests.rs"]
pub(crate) mod tests;
