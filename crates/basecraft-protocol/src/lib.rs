//! Shared types, operation schemas and the plugin protocol.
//!
//! Apache-2.0, so plugins and the Python SDK can use any license.
//! This crate must never depend on a GPL crate.

pub mod annotation;
pub mod companion_records;
pub mod delimitation;
pub mod export_records;
pub mod input_records;
pub mod installation_records;
pub mod link_records;
pub mod metadata;
pub mod metadata_records;
pub mod operation;
pub mod ops;
pub mod plugin;
pub mod producer_records;
pub mod provenance;
pub mod publication_records;
pub mod query_records;
pub mod records;
pub mod report_records;
pub mod sanger;
pub mod sanger_records;
pub mod submission;
pub mod submission_records;
pub mod tree;

pub use annotation::{Feature, Location, Qualifier, Segment, Strand};
pub use ops::{Actor, DocState, LogEntry, Op, OpResult};
pub use tree::{
    CladeAnnotation, CladeRef, SupportKind, SupportPlacement, SupportSymbols, TreeStyle,
};

pub mod distance_records;

pub mod sanderson_records;
