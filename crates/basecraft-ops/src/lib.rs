//! Operation registry, undo and redo, the operation log and replay.

pub mod files;
pub mod log;
pub mod replay;
pub mod session;

mod error;

pub use error::{Error, Result};
pub use session::{DocId, Session};
