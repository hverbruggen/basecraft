//! Kind-specific behavior borrowed by the shared Session.
//!
//! Migration is incremental: Submission, Metadata, Alignment and Tree implement this interface today.
//! `Loaded::as_document` returns `None` for legacy kinds. Their existing paths
//! remain until each is migrated; no placeholder implementations are required.
//! Payloads are not constrained to a single sequence/record, so future result,
//! multi-record and Sanger documents can retain their own models here.

use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_protocol::{DocState, Op};

use crate::Result;
use crate::files::Format;
use crate::session::Inverse;

mod alignment;
mod metadata;
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
    /// partial-save recovery and publication. Paths remain absolute/as supplied;
    /// Session converts them to project-relative log keys. The recovery root is
    /// diagnostic context for partial multi-file saves, not owned by the payload.
    fn write(
        &self,
        target: &Path,
        format: Format,
        recovery_root: &Path,
    ) -> Result<Vec<(PathBuf, String)>>;
    /// Preserve the existing replay writer, including historical target handling.
    fn saved_hashes(&self, target: &Path) -> Result<Vec<String>>;
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
