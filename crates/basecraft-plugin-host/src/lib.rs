//! Plugin loading, isolation and transport.
//!
//! Plugins run in owned processes and talk JSON-RPC over a Unix socket or
//! authenticated Windows loopback connection.
//! Documents reach them as sealed shared-memory Arrow buffers; they answer
//! with proposed operations, which the ops session applies and records as a
//! proposal (`Session::apply_sequence_proposals`).

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
#[cfg(windows)]
mod windows_transport;

mod error;

pub use error::{Error, Result};
pub use manifest::Manifest;
pub use process::PluginProcess;
pub use snapshot::Snapshot;
