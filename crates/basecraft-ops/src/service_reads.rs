//! Finite application-owned reads. Unwired: a context is configuration, not permission.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::access::{Access, AccessError, Denial, ResourceAction, ServiceAction};
use basecraft_platform::{
    files::{
        CheckedRead, LockDirectoryMode, LockNamespace, LockStep, NamespaceReadError, OpenedLock,
        ReadNamespace, TargetNamespace, inspect_lock_namespace, inspect_read_namespace,
        namespace_lock_supported, namespace_read_supported,
    },
    paths::AppPaths,
    settings::{Preferences, SettingsStore},
};

pub type ServiceReadContext = ServiceContext;
pub type ServiceReadRequest<'a> = ServiceRequest<'a>;
pub type ServiceReadPhase<'a> = ServicePhase<'a>;

pub struct ServiceContext {
    preferences: PathBuf,
    registry: PathBuf,
    evidence: PathBuf,
    reservations: PathBuf,
}

/// Only the finite owner constructs requests; a wire path/action is not a grant.
/// ```compile_fail
/// use basecraft_ops::{access::ServiceAction, service_reads::{ServiceContext, ServiceRequest}};
/// fn forge(context: &ServiceContext) {
///     let request = ServiceRequest { context, action: ServiceAction::ReadPreferences };
/// }
/// ```
pub struct ServiceRequest<'a> {
    context: &'a ServiceContext,
    action: ServiceAction,
    target: Option<(&'a Path, SaveTargetPurpose)>,
}

/// Borrows the exact request, so a ticket cannot outlive or move its request.
/// A ticket is consumed once, and cannot escape the live request lifetime.
/// ```compile_fail
/// use basecraft_ops::service_reads::CheckedServiceNamespace;
/// fn replay(ticket: CheckedServiceNamespace<'_>) {
///     let first = ticket;
///     let second = ticket;
///     drop((first, second));
/// }
/// ```
/// ```compile_fail
/// use basecraft_ops::service_reads::CheckedServiceNamespace;
/// fn escape<'a>(ticket: CheckedServiceNamespace<'a>) -> CheckedServiceNamespace<'static> {
///     ticket
/// }
/// ```
pub struct CheckedServiceNamespace<'a> {
    request: &'a ServiceRequest<'a>,
    namespace: ReadNamespace,
}

pub enum ServicePhase<'a> {
    Eligibility,
    Validated(&'a CheckedServiceNamespace<'a>),
    LockValidated(&'a CheckedLockNamespace<'a>),
    BeforeEffect(&'a CheckedLockNamespace<'a>, ServiceEffect),
    Held(&'a CheckedLockNamespace<'a>),
}

impl ServiceRequest<'_> {
    /// Pure policy predicate; no path resolution, operation verification or IO.
    pub fn permits(&self, context: &ServiceContext, phase: &ServicePhase<'_>) -> bool {
        self.target.is_none()
            && std::ptr::eq(self.context, context)
            && matches!(
                self.action,
                ServiceAction::ReadPreferences | ServiceAction::ReadAnalysisRegistry
            )
            && namespace_read_supported()
            && match phase {
                ServicePhase::Eligibility => true,
                ServicePhase::Validated(ticket) => std::ptr::eq(self, ticket.request),
                _ => false,
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
        if !self.permits(self.context, &ServicePhase::Eligibility) {
            return Err(AccessError::Denied(Denial::ServiceUnavailable).into());
        }
        let location = self.location()?;
        access.service_read(self, ServicePhase::Eligibility)?;
        let ticket = CheckedServiceNamespace {
            request: self,
            namespace: inspect(location).map_err(namespace_error)?,
        };
        access.service_read(self, ServicePhase::Validated(&ticket))?;
        Ok(consume(ticket.namespace).map_err(namespace_error)?)
    }
}

impl ServiceContext {
    /// Capture existing process configuration once. Discovery performs no IO.
    pub fn from_process() -> crate::Result<Self> {
        let paths = AppPaths::discover().map_err(|e| crate::error::invalid(e.to_string()))?;
        Self::from_paths(&paths)
    }

    fn from_paths(paths: &AppPaths) -> crate::Result<Self> {
        let context = Self {
            preferences: paths.preferences(),
            registry: crate::input_evidence::registry_location(paths),
            evidence: crate::input_evidence::evidence_lock_path(&paths.state),
            reservations: crate::save_guard::lock_root(),
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
        let input = ServiceRequest {
            context: self,
            action: ServiceAction::ReadPreferences,
            target: None,
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

/// Routing purpose only, never saved-byte consent or `RecoveryAttempt` ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveTargetPurpose {
    NewFile,
    ReplaceFile,
    StageFile,
    FinalDirectory,
}
impl SaveTargetPurpose {
    pub fn action(self) -> ResourceAction {
        match self {
            Self::NewFile | Self::StageFile => ResourceAction::CreateFile,
            Self::ReplaceFile => ResourceAction::ReplaceFile,
            Self::FinalDirectory => ResourceAction::RenameDestination,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceEffect {
    CreateDirectory,
    SetPermissions,
    OpenFile,
    AcquireLock,
}

/// The host's response is tied to the same live request. No wire constructor or
/// independently reusable target ticket. Host Access implementations are trusted
/// policy owners, and must supply their actual strict resolver observation.
/// ```compile_fail
/// use basecraft_ops::service_reads::CheckedServiceTarget;
/// fn escape<'a>(target: CheckedServiceTarget<'a>) -> CheckedServiceTarget<'static> { target }
/// ```
pub struct CheckedServiceTarget<'a> {
    request: &'a ServiceRequest<'a>,
    observation: TargetObservation,
}
struct TargetObservation {
    requested: PathBuf,
    purpose: SaveTargetPurpose,
    path: PathBuf,
    namespace: TargetNamespace,
}

enum LockObservation<'a> {
    Pending(&'a LockNamespace),
    Opened(&'a OpenedLock),
}
/// Metadata validation state, borrowing the exact request. Not an authority
/// receipt, serializable token or an independently constructible capability.
/// ```compile_fail
/// use basecraft_ops::service_reads::CheckedLockNamespace;
/// fn escape<'a>(ticket: CheckedLockNamespace<'a>) -> CheckedLockNamespace<'static> { ticket }
/// ```
pub struct CheckedLockNamespace<'a> {
    request: &'a ServiceRequest<'a>,
    target: Option<&'a TargetObservation>,
    lock: LockObservation<'a>,
}
impl CheckedLockNamespace<'_> {
    /// The retained resolved target, not a relative spelling reinterpreted under
    /// a different policy root during guard reauthorization.
    pub fn target(&self) -> Option<(&Path, ResourceAction)> {
        self.target
            .map(|target| (target.path.as_path(), target.purpose.action()))
    }
    fn recheck(&self) -> crate::access::Result<()> {
        if let Some(target) = self.target {
            target.namespace.recheck().map_err(namespace_error)?;
        }
        match self.lock {
            LockObservation::Pending(lock) => lock.recheck(),
            LockObservation::Opened(lock) => lock.recheck(),
        }
        .map_err(namespace_error)
    }
}

impl ServiceRequest<'_> {
    pub fn target(&self) -> Option<(&Path, ResourceAction)> {
        self.target.map(|(path, purpose)| (path, purpose.action()))
    }
    /// Bind an actual host strict observation to this exact request. This is a
    /// policy implementation interface, not a path-based authorization API.
    pub fn checked_target(
        &self,
        path: PathBuf,
        namespace: TargetNamespace,
    ) -> crate::access::Result<CheckedServiceTarget<'_>> {
        let Some((requested, purpose)) = self.target else {
            return Err(unavailable());
        };
        if self.action != ServiceAction::ReserveSaveTarget {
            return Err(unavailable());
        }
        Ok(CheckedServiceTarget {
            request: self,
            observation: TargetObservation {
                requested: requested.to_owned(),
                purpose,
                path,
                namespace,
            },
        })
    }
    /// Pure finite predicate. Host separately checks both write flags and the
    /// same resource lexical/action policy before every effect; no IO here.
    pub fn permits_lock(&self, context: &ServiceContext, phase: &ServicePhase<'_>) -> bool {
        if !std::ptr::eq(self.context, context)
            || !namespace_lock_supported()
            || !self.context.lock_locations_supported()
            || !matches!(
                (self.action, self.target),
                (ServiceAction::LockAnalysisEvidence, None)
                    | (ServiceAction::ReserveSaveTarget, Some(_))
            )
        {
            return false;
        }
        let ticket = match phase {
            ServicePhase::Eligibility => return true,
            ServicePhase::LockValidated(ticket)
            | ServicePhase::BeforeEffect(ticket, _)
            | ServicePhase::Held(ticket) => ticket,
            ServicePhase::Validated(_) => return false,
        };
        if !std::ptr::eq(self, ticket.request) {
            return false;
        }
        match (self.target, ticket.target) {
            (None, None) => {}
            (Some((path, purpose)), Some(target))
                if path == target.requested && purpose == target.purpose => {}
            _ => return false,
        }
        match (phase, &ticket.lock) {
            (ServicePhase::BeforeEffect(_, effect), LockObservation::Pending(lock)) => {
                matches!(
                    (effect, lock.next_step()),
                    (
                        ServiceEffect::CreateDirectory,
                        Ok(LockStep::CreateDirectory)
                    ) | (ServiceEffect::SetPermissions, Ok(LockStep::SetPermissions))
                        | (ServiceEffect::OpenFile, Ok(LockStep::OpenFile))
                )
            }
            (ServicePhase::LockValidated(_), LockObservation::Pending(_))
            | (
                ServicePhase::BeforeEffect(_, ServiceEffect::AcquireLock) | ServicePhase::Held(_),
                LockObservation::Opened(_),
            ) => true,
            _ => false,
        }
    }
    fn authorize_lock(
        &self,
        access: &dyn Access,
        phase: ServicePhase<'_>,
    ) -> crate::access::Result<()> {
        if !self.permits_lock(self.context, &phase) {
            return Err(unavailable());
        }
        access.service_context(self, phase)
    }
}
fn unavailable() -> AccessError {
    AccessError::Denied(Denial::ServiceUnavailable)
}

/// The acquisition request never escapes. The returned owner retains immutable
/// configuration and structural observations, not a consumed authorization.
/// Reauthorization mints a fresh borrowed request over exactly this retained
/// state, calls the bound policy, then rechecks WITHOUT inspection/rebootstrap.
#[allow(dead_code)]
pub(crate) struct ServiceLockGuard {
    context: Arc<ServiceContext>,
    action: ServiceAction,
    target: Option<TargetObservation>,
    opened: OpenedLock,
}
#[allow(dead_code)]
impl ServiceLockGuard {
    pub(crate) fn reauthorize(&self, access: &dyn Access) -> crate::Result<()> {
        let request = ServiceRequest {
            context: &self.context,
            action: self.action,
            target: self
                .target
                .as_ref()
                .map(|target| (target.requested.as_path(), target.purpose)),
        };
        let ticket = CheckedLockNamespace {
            request: &request,
            target: self.target.as_ref(),
            lock: LockObservation::Opened(&self.opened),
        };
        request.authorize_lock(access, ServicePhase::Held(&ticket))?;
        ticket.recheck()?;
        Ok(())
    }
}

#[allow(dead_code)]
pub(crate) fn acquire_lock(
    context: Arc<ServiceContext>,
    access: &dyn Access,
    target: Option<(&Path, SaveTargetPurpose)>,
) -> crate::Result<ServiceLockGuard> {
    let action = if target.is_some() {
        ServiceAction::ReserveSaveTarget
    } else {
        ServiceAction::LockAnalysisEvidence
    };
    let request = ServiceRequest {
        context: &context,
        action,
        target,
    };
    request.authorize_lock(access, ServicePhase::Eligibility)?;
    let target = if target.is_some() {
        let checked = access.service_target(&request)?;
        if !std::ptr::eq(checked.request, std::ptr::from_ref(&request)) {
            return Err(unavailable().into());
        }
        Some(checked.observation)
    } else {
        None
    };
    let (path, mode) = if let Some(target) = &target {
        // A strict ordinary no-alias absolute spelling is the legacy canonical
        // spelling. No canonicalize/reopen after the retained host observation.
        (
            crate::save_guard::lock_path(&context.reservations, &target.path),
            LockDirectoryMode::Private,
        )
    } else {
        (context.evidence.clone(), LockDirectoryMode::Preserve)
    };
    let mut namespace = inspect_lock_namespace(&path, mode).map_err(namespace_error)?;
    {
        let ticket = CheckedLockNamespace {
            request: &request,
            target: target.as_ref(),
            lock: LockObservation::Pending(&namespace),
        };
        request.authorize_lock(access, ServicePhase::LockValidated(&ticket))?;
        ticket.recheck()?;
    }
    let opened = loop {
        let step = namespace.next_step().map_err(namespace_error)?;
        let effect = match step {
            LockStep::CreateDirectory => ServiceEffect::CreateDirectory,
            LockStep::SetPermissions => ServiceEffect::SetPermissions,
            LockStep::OpenFile => ServiceEffect::OpenFile,
        };
        {
            let ticket = CheckedLockNamespace {
                request: &request,
                target: target.as_ref(),
                lock: LockObservation::Pending(&namespace),
            };
            request.authorize_lock(access, ServicePhase::BeforeEffect(&ticket, effect))?;
            ticket.recheck()?;
        }
        namespace = match step {
            LockStep::CreateDirectory => namespace.create_next_directory(),
            LockStep::SetPermissions => namespace.set_next_permissions(),
            LockStep::OpenFile => break namespace.open_file().map_err(namespace_error)?,
        }
        .map_err(namespace_error)?;
    };
    {
        let ticket = CheckedLockNamespace {
            request: &request,
            target: target.as_ref(),
            lock: LockObservation::Opened(&opened),
        };
        request.authorize_lock(
            access,
            ServicePhase::BeforeEffect(&ticket, ServiceEffect::AcquireLock),
        )?;
        ticket.recheck()?;
        #[cfg(all(test, target_os = "linux"))]
        tests::locks::advisory_boundary();
        if action == ServiceAction::ReserveSaveTarget {
            crate::save_guard::require_reservation(
                opened.try_lock_exclusive().map_err(namespace_error)?,
            )?;
        } else {
            opened.lock_exclusive().map_err(namespace_error)?;
        }
        request.authorize_lock(access, ServicePhase::Held(&ticket))?;
        ticket.recheck()?;
    }
    Ok(ServiceLockGuard {
        context,
        action,
        target,
        opened,
    })
}

// Finite owner entry points; no application/adapter route constructs the bound
// policy yet. Arc retains immutable configuration, never an authorization ticket.
pub use crate::save_guard::Reservation;
impl ServiceContext {
    // Pure finite service-configuration shape check. The lower namespace walker
    // still owns all structural/ownership checks; this does no path resolution.
    fn lock_locations_supported(&self) -> bool {
        [&self.evidence, &self.reservations].iter().all(|path| {
            let bytes = path.as_os_str().as_encoded_bytes();
            bytes.starts_with(b"/")
                && !bytes.starts_with(b"//")
                && !bytes.ends_with(b"/")
                && !bytes.contains(&0)
                && !bytes
                    .split(|byte| *byte == b'/')
                    .skip(1)
                    .any(|part| part.is_empty() || part == b"." || part == b"..")
        })
    }
    pub fn reserve_target(
        self: &Arc<Self>,
        access: &dyn Access,
        target: &Path,
        purpose: SaveTargetPurpose,
    ) -> crate::Result<Reservation> {
        crate::save_guard::lock_with_access(Arc::clone(self), access, target, purpose)
    }
    pub fn lock_evidence(
        self: &Arc<Self>,
        access: &dyn Access,
    ) -> crate::Result<crate::input_evidence::Mutation> {
        crate::input_evidence::Mutation::begin_with_access(Arc::clone(self), access)
    }
}
