//! Kind-specific behavior borrowed by the shared Session.
//!
//! Seven payloads share identity, journal, undo and save ownership. Each payload
//! supplies its own edits and serializer; scientific review history stays specialised.

use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op};

use crate::Result;
use crate::files::Format;
use crate::session::Inverse;

mod alignment;
mod delimitation;
mod metadata;
mod sanger;
mod sequence;
mod submission;
mod tree;

pub(crate) trait Document {
    fn kind(&self) -> crate::files::Kind;
    /// Reject operations for another payload before durable tracking starts.
    fn accepts(&self, op: &Op) -> bool;
    /// Whether admission must retain the discovered companion hashes.
    fn retains_companions(&self) -> bool {
        false
    }
    fn opening_warning(&self) -> Option<&str> {
        None
    }

    fn rebase_replay_paths(&mut self, _from: &Path, _to: &Path) {}
    /// Detached proposal staging retains the same payload and writer metadata.
    /// Unsupported kinds cannot be mutated through the proposal route.
    fn proposal_candidate(&self) -> Result<crate::files::Loaded> {
        Err(crate::Error::Unsupported(
            "this document kind does not support proposals".into(),
        ))
    }
    fn apply_proposal(&mut self, _op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        Err(crate::Error::Unsupported(
            "this document kind does not support proposals".into(),
        ))
    }
    fn revision(&self, generation: u64) -> u64;
    fn fingerprint(&self) -> DocState;
    /// Apply transactionally: an error must leave the payload unchanged.
    /// Session owns actor, history, journal and external input verification.
    fn validate(&self, _op: &Op, _root: &Path, _live: bool) -> Result<()> {
        Ok(())
    }
    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)>;
    fn revert(&mut self, inverse: &Inverse) -> Result<()>;
    /// Redo using the original inverse when an edit allocated stable identities.
    /// Session still owns stacks and the journal; payloads only restore the edit.
    fn reapply(&mut self, op: &Op, _inverse: &Inverse) -> Result<Inverse> {
        self.apply(op).map(|(inverse, _)| inverse)
    }

    /// Write using atomic replacement per file. Session owns save guards,
    /// partial-save recovery and document provenance publication. Kind-specific
    /// analysis records use the existing record owner from the writer. Paths remain absolute/as supplied;
    /// Session converts them to project-relative log keys. The recovery root is
    /// diagnostic context for partial multi-file saves, not owned by the payload.
    /// Prepare linked evidence when saving a document to another location.
    /// Session publishes it with the guarded document write and records outputs.
    fn prepare_links(
        &self,
        _original: &Path,
        _target: &Path,
        _format: Format,
    ) -> Result<Option<crate::links::PreparedLinks>> {
        Ok(None)
    }
    fn write(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
        publication: &WritePublication,
    ) -> Result<Vec<(PathBuf, String)>>;
    /// Files replaced independently before the primary file. Recovery accepts
    /// each matching expected hash even when the primary replacement failed.
    /// Ordering matches `saved_hashes` (metadata includes its manifest here).
    fn independently_saved_files(&self, _target: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Companion accepted only after the main file matches the intended write.
    /// Its expected hash is the second element of `saved_hashes`.
    fn saved_companion(&self, _target: &Path) -> Option<PathBuf> {
        None
    }

    /// Whether saved hashes and their errors depend only on the complete payload.
    /// Opt-in requires stable transitive serializer inputs between Slot mutable
    /// accesses: no target, publication, filesystem, time or shared payload mutation.
    /// Current opted-in Session constructors use owned backing or immutable indexed
    /// FASTA backing. The public `Residues` trait and `Source::Mapped` alone do not
    /// guarantee this invariant; future constructors must preserve it or opt out.
    fn saved_hashes_are_content_only(&self) -> bool {
        false
    }

    /// Hash the current writer output for this destination.
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

/// Placement only: no consent, policy, source or overwrite authority enters a writer.
#[derive(Default)]
pub(crate) enum WritePublication {
    #[default]
    Immediate,
    /// The owner accepted an absent destination; promotion must not replace another writer.
    Create,
    Recovery {
        stage: PathBuf,
        destination: PathBuf,
        stores: std::sync::Mutex<Vec<crate::analysis_store::ArtifactRun>>,
    },
}

impl WritePublication {
    pub(crate) fn logical(&self, path: &Path) -> Result<PathBuf> {
        match self {
            Self::Immediate | Self::Create => Ok(path.into()),
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
            Self::Immediate | Self::Create => crate::input_evidence::register(run),
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
