//! Operation registry, undo and redo, the operation log and replay.

pub mod access;
pub mod alignment_add;
pub mod analysis_store;
pub mod blast;
pub mod files;
pub mod input_evidence;
pub mod links;
pub mod log;
pub mod metadata;
pub mod provenance;
pub mod replay;
pub mod sanger_review;
pub mod service_reads;
pub mod session;
pub mod submission;

mod documents;
mod error;
mod save_guard;

pub use error::{Error, Result};
pub use session::{DocId, Session};

pub mod document_record;

pub mod delimitation;

mod contributions;

#[cfg(test)]
#[path = "../tests/support/isolated_profile.rs"]
mod isolated_profile;
