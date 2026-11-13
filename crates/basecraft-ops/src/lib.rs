//! Operation registry, undo and redo, the operation log and replay.

pub mod access;
pub mod alignment_add;
pub mod blast;
pub mod files;
pub mod log;
pub mod metadata;
pub mod project;
pub mod records;
pub mod renames;
pub mod sanger_review;
pub mod session;
pub mod submission;
pub mod work;

mod documents;
mod error;

pub use error::{Error, Result};
pub use session::{DocId, Session};

pub mod delimitation;

#[cfg(test)]
#[path = "../tests/support/isolated_profile.rs"]
mod isolated_profile;
