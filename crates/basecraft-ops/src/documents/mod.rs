//! Kind-specific behavior borrowed by the shared Session.
//!
//! Migration is incremental: only Submission implements this interface today.
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

mod submission;

pub(crate) trait Document {
    fn revision(&self, generation: u64) -> u64;
    fn fingerprint(&self) -> DocState;
    /// Apply transactionally: an error must leave the payload unchanged.
    /// Session owns actor, history, journal and external input verification.
    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)>;
    fn revert(&mut self, inverse: &Inverse) -> Result<()>;
    /// Write using atomic replacement per file. Session owns save guards,
    /// partial-save recovery and publication. Paths remain absolute/as supplied;
    /// Session converts them to project-relative log keys.
    fn write(&self, target: &Path, format: Format) -> Result<Vec<(PathBuf, String)>>;
    /// Preserve the existing replay writer, including historical target handling.
    fn saved_hashes(&self, target: &Path) -> Result<Vec<String>>;
}

// Discovery precedes payload loading, so it cannot borrow a Document yet.
pub(crate) use submission::companions as submission_companions;
