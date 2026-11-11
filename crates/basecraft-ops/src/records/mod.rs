//! Basecraft's own records about documents and analyses: provenance
//! publications and bundles, alignment links, document History and Methods,
//! and journal replay and recovery. Other crates use the names exported here.

pub(crate) mod contributions;
pub(crate) mod document_record;
pub(crate) mod links;
pub(crate) mod provenance;
pub(crate) mod replay;

pub use document_record::{DocumentRecord, MethodsParagraph, Report, ReportExt, jobs};
pub use links::{AlignmentLink, AlignmentLinkExt, metadata_present, sidecar, view_sidecar};
pub use provenance::{
    bundle_for_result, companion, inspect, package, publish_operation, read_record, verify,
    write_reports,
};
pub use replay::{
    PublishedWithFailure, RecoveryCompletion, RecoveryMode, RecoveryPhase, ReplayReport,
    prepare_recovery, recover, recover_verified_prefix, replay,
};
