//! Shared types, operation schemas and the plugin protocol.
//!
//! Apache-2.0, so plugins and the Python SDK can use any license.
//! This crate must never depend on a GPL crate.

pub mod annotation;
pub mod ops;
pub mod plugin;
pub mod tree;

pub use annotation::{Feature, Location, Qualifier, Segment, Strand};
pub use ops::{Actor, DocState, LogEntry, Op, OpResult};
pub use tree::{CladeAnnotation, CladeRef, SupportPlacement, SupportSymbols, TreeStyle};
