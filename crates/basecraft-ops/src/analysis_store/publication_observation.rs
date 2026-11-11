//! Passive facts for one secondary output, not a receipt or a completion owner.
//! The source artifact is not pinned by these facts. Nothing here performs IO
//! on drop, retries publication, or decides cleanup/recovery policy.
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

use crate::{Error, Result};
use basecraft_platform::files::{
    ObjectIdentity, OrdinaryKind, handle_object_identity, object_identity, open_regular_read,
};

#[derive(Debug)]
pub(crate) struct StagedWitness {
    identity: ObjectIdentity,
    hash: String,
    size: u64,
}
impl StagedWitness {
    pub(crate) fn identity(&self) -> &ObjectIdentity {
        &self.identity
    }
    pub(crate) fn hash(&self) -> &str {
        &self.hash
    }
    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}

#[derive(Debug)]
pub(crate) enum PersistFact {
    NotAttempted {
        staged: Option<StagedWitness>,
    },
    /// No successful-return witness; NOT proof of rollback or absence.
    NoCommitWitness {
        staged: StagedWitness,
    },
    Committed {
        witness: StagedWitness,
    },
}
impl PersistFact {
    pub(crate) fn witness(&self) -> Option<&StagedWitness> {
        match self {
            Self::NotAttempted { staged } => staged.as_ref(),
            Self::NoCommitWitness { staged } => Some(staged),
            Self::Committed { witness } => Some(witness),
        }
    }
}

#[derive(Debug)]
pub(crate) enum TargetObservation {
    NotRequested,
    Absent,
    Present {
        identity: ObjectIdentity,
        hash: String,
        size: u64,
    },
    Unavailable,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParentSync {
    NotAttempted,
    Complete,
    Failed,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ObservationPhase {
    PostWriteInspection,
}
#[derive(Debug)]
pub(crate) struct ObservationFailure {
    phase: ObservationPhase,
    cause: Error,
}
impl ObservationFailure {
    pub(crate) fn phase(&self) -> &ObservationPhase {
        &self.phase
    }
    pub(crate) fn cause(&self) -> &Error {
        &self.cause
    }
}
#[derive(Debug)]
pub(crate) struct FilePublicationObservation {
    // Caller spelling only; this is never a validated path or access grant.
    destination: PathBuf,
    persist: PersistFact,
    parent_sync: ParentSync,
    target: TargetObservation,
}
impl FilePublicationObservation {
    pub(crate) fn new(destination: &Path) -> Self {
        Self {
            destination: destination.into(),
            persist: PersistFact::NotAttempted { staged: None },
            parent_sync: ParentSync::NotAttempted,
            target: TargetObservation::NotRequested,
        }
    }
    pub(crate) fn staged(&mut self, file: &File, hash: String, size: u64) -> io::Result<()> {
        let identity = handle_object_identity(file, OrdinaryKind::File)?;
        self.persist = PersistFact::NotAttempted {
            staged: Some(StagedWitness {
                identity,
                hash,
                size,
            }),
        };
        Ok(())
    }
    pub(crate) fn attempting(&mut self) {
        let prior = std::mem::replace(
            &mut self.persist,
            PersistFact::NotAttempted { staged: None },
        );
        self.persist = match prior {
            PersistFact::NotAttempted {
                staged: Some(staged),
            } => PersistFact::NoCommitWitness { staged },
            other => other,
        };
    }
    pub(crate) fn persisted(&mut self) {
        let prior = std::mem::replace(
            &mut self.persist,
            PersistFact::NotAttempted { staged: None },
        );
        self.persist = match prior {
            PersistFact::NoCommitWitness { staged } => PersistFact::Committed { witness: staged },
            other => other,
        };
    }
    pub(crate) fn inspected(
        &mut self,
        result: Result<TargetObservation>,
    ) -> Option<ObservationFailure> {
        match result {
            Ok(target) => {
                self.target = target;
                None
            }
            Err(cause) => {
                self.target = TargetObservation::Unavailable;
                Some(ObservationFailure {
                    phase: ObservationPhase::PostWriteInspection,
                    cause,
                })
            }
        }
    }
    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }
    pub(crate) fn persist(&self) -> &PersistFact {
        &self.persist
    }
    pub(crate) fn parent_sync(&self) -> &ParentSync {
        &self.parent_sync
    }
    pub(crate) fn target(&self) -> &TargetObservation {
        &self.target
    }
}
/// Strict path identities bracket same-handle hashing and full identity. This
/// rejects stable leaf links, but is not an atomic snapshot or hostile-mutator
/// confinement: ancestors and content may change, and paths can rebind later.
pub(crate) fn inspect_target(path: &Path) -> Result<TargetObservation> {
    let before = match object_identity(path, OrdinaryKind::File) {
        Ok(identity) => identity,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TargetObservation::Absent);
        }
        Err(error) => return Err(error.into()),
    };
    let mut file = open_regular_read(path)?;
    let identity = handle_object_identity(&file, OrdinaryKind::File)?;
    let mut hashing = crate::files::HashingWriter::new(io::sink());
    let size = io::copy(&mut file, &mut hashing)?;
    let after = object_identity(path, OrdinaryKind::File)?;
    if before != identity || identity != after {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "secondary output identity changed during observation",
        )
        .into());
    }
    Ok(TargetObservation::Present {
        identity,
        hash: hashing.hash(),
        size,
    })
}
