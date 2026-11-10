//! Kind-specific behavior borrowed by the shared Session.
//!
//! Migration is incremental: Submission, Metadata, Alignment, Tree, Sequence and Delimitation implement this interface today.
//! All six existing payloads participate; Sanger review integration remains separate.
//! Payloads are not constrained to a single sequence/record, so future result,
//! multi-record and Sanger documents can retain their own models here.

use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op};

use crate::Result;
use crate::files::Format;
use crate::session::Inverse;

mod alignment;
mod delimitation;
mod metadata;
#[cfg(test)]
pub(crate) use metadata::observation::{Hooks as DocumentHooks, MetadataFacts};
#[cfg(test)]
pub(crate) struct DocumentAttempt {
    pub result: Result<Vec<(PathBuf, String)>>,
    pub metadata: Option<MetadataFacts>,
}
mod sequence;
mod submission;
mod tree;

pub(crate) trait Document {
    fn revision(&self, generation: u64) -> u64;
    fn fingerprint(&self) -> DocState;
    /// Apply transactionally: an error must leave the payload unchanged.
    /// Session owns actor, history, journal and external input verification.
    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)>;
    fn revert(&mut self, inverse: &Inverse) -> Result<()>;
    /// Write using atomic replacement per file. Session owns save guards,
    /// partial-save recovery and document provenance publication. Kind-specific
    /// analysis records use the existing record owner from the writer. Paths remain absolute/as supplied;
    /// Session converts them to project-relative log keys. The recovery root is
    /// diagnostic context for partial multi-file saves, not owned by the payload.
    fn write(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
        publication: &WritePublication,
    ) -> Result<Vec<(PathBuf, String)>>;
    #[cfg(test)]
    fn write_observed(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
        publication: &WritePublication,
        _hooks: DocumentHooks,
    ) -> DocumentAttempt {
        DocumentAttempt {
            result: self.write(target, format, recovery_root, publication),
            metadata: None,
        }
    }
    /// Preserve the existing replay writer, including historical target handling.
    fn saved_hashes(&self, target: &Path, publication: &WritePublication) -> Result<Vec<String>>;
    /// Kind-specific operation composition. Session owns timing, actor/group
    /// boundaries, pending history and stitching the existing inverse steps.
    fn merge(&self, _previous: &Op, _next: &Op) -> Option<Op> {
        None
    }
}

// Discovery precedes payload loading, so it cannot borrow a Document yet.
pub(crate) use submission::companions as submission_companions;

// Shared by staged support import and drawing-edit replay on detached TreeDocs.
pub(crate) use tree::execute_tree;

// Detached proposal staging uses the same sequence semantics as live documents.
pub(crate) use sequence::execute_sequence;

// Detached proposals share Delimitation mutations and fingerprints with Session.
pub(crate) use delimitation::execute_delimitation;

/// Placement only: no consent, policy, source or overwrite authority enters a writer.
#[derive(Default)]
pub(crate) enum WritePublication {
    #[default]
    Immediate,
    Recovery {
        stage: PathBuf,
        destination: PathBuf,
        stores: std::sync::Mutex<Vec<crate::analysis_store::ArtifactRun>>,
    },
}

impl WritePublication {
    pub(crate) fn logical(&self, path: &Path) -> Result<PathBuf> {
        match self {
            Self::Immediate => Ok(path.into()),
            Self::Recovery {
                stage, destination, ..
            } => Ok(destination.join(
                path.strip_prefix(stage)
                    .map_err(|_| crate::error::invalid("recovery write lies outside stage"))?,
            )),
        }
    }

    pub(crate) fn register(&self, run: &crate::analysis_store::ArtifactRun) -> Result<()> {
        match self {
            Self::Immediate => crate::input_evidence::register(run),
            Self::Recovery { stores, .. } => {
                let mut stores = stores
                    .lock()
                    .map_err(|_| crate::error::invalid("recovery stores unavailable"))?;
                if !stores.iter().any(|old| old.directory() == run.directory()) {
                    stores.push(run.clone());
                }
                Ok(())
            }
        }
    }
}
