//! Plugin loading, isolation and transport.
//!
//! Plugins run in their own processes and talk JSON-RPC over a Unix socket.
//! Documents reach them as sealed shared-memory Arrow buffers; they answer
//! with proposed operations, which [`apply::apply_proposals`] maps onto the
//! current document version and applies through the ops session.

pub mod apply;
pub mod compute;
pub mod contract;
mod delimitation_pipeline;
pub mod file_tool;
pub mod installation;
pub mod jobs;
mod lifecycle;
pub mod manifest;
pub mod policy;
pub mod process;
pub mod registry;
pub mod runner;
pub mod runtime;
pub mod sanger_calls;
pub mod snapshot;

mod error;

pub use error::{Error, Result};
pub use manifest::Manifest;
pub use process::PluginProcess;
pub use snapshot::Snapshot;
