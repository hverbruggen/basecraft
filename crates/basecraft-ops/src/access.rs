//! Caller access boundary for retained workflows. No filesystem IO lives here.
//!
//! This foundation is not yet wired into recovery. Owners must preflight the
//! complete mutation set, then check again immediately before each effect.
//! A returned path is a checked spelling, not a durable capability or receipt.
//! Denials must propagate before optional-data warnings or fallback conversion.
use std::path::{Path, PathBuf};

/// Each metadata/existence probe is an access too. Create and rename destination
/// require a missing leaf with an existing parent; they never authorize overwrite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceAction {
    Probe,
    ReadFile,
    ReadDirectory,
    CreateFile,
    CreateDirectory,
    ReplaceFile,
    RenameSource,
    RenameDestination,
    Cleanup,
    SyncFile,
    SyncDirectory,
}

impl ResourceAction {
    pub fn writes(self) -> bool {
        !matches!(self, Self::Probe | Self::ReadFile | Self::ReadDirectory)
    }

    pub fn creates(self) -> bool {
        matches!(
            self,
            Self::CreateFile | Self::CreateDirectory | Self::RenameDestination
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Denial {
    #[error("path must not be empty or contain parent traversal")]
    InvalidPath,
    #[error("path lies outside session root")]
    OutsideRoot,
    #[error("operation requires both document and artifact writes")]
    ReadOnly,
    #[error("symbolic links are not allowed")]
    SymbolicLink,
    #[error("file has hardlink aliases")]
    Hardlink,
    #[error("resource has an unsupported type")]
    ResourceType,
    #[error("the session root cannot be replaced, moved or removed")]
    RootMutation,
}

/// Kept distinct from ordinary IO and from the existing ops error taxonomy.
/// Propagation must preserve this type; never inspect message text for denial.
#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("permission_denied: {0}")]
    Denied(Denial),
    #[error("output_conflict: destination already exists")]
    DestinationExists,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AccessError>;

pub trait Access {
    /// Discovery must stop here BEFORE probing a parent outside this boundary.
    /// None is reserved for explicitly trusted legacy entry points.
    fn boundary(&self) -> Option<&Path>;

    /// Check one resource using the caller's existing path/effect owner. Cleanup
    /// additionally requires the attempt's actual prepublication stage ownership;
    /// path permission alone never authorizes deleting a project subtree.
    fn resource(&self, path: &Path, action: ResourceAction) -> Result<PathBuf>;
}

/// Explicit compatibility mode for trusted legacy entry points only. No Default
/// implementation: MCP must bind its `SessionPolicy`, never choose this implicitly.
/// This does not bypass domain safety, source checks or no-replace publication.
pub struct TrustedLegacy;

impl Access for TrustedLegacy {
    fn boundary(&self) -> Option<&Path> {
        None
    }

    fn resource(&self, path: &Path, _action: ResourceAction) -> Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}
