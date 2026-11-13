//! Basecraft's own records about documents and analyses: provenance
//! publications and bundles, alignment links, document History and Methods,
//! journal replay and recovery, analysis runs and the evidence of their
//! inputs, and save guards. Other crates use the names exported here.

pub(crate) mod analysis_store;
pub(crate) mod contributions;
pub(crate) mod document_record;
pub(crate) mod input_evidence;
pub(crate) mod jobs;
pub(crate) mod links;
pub(crate) mod provenance;
pub(crate) mod publish;
pub(crate) mod replay;
pub(crate) mod save_guard;

pub use analysis_store::{ArtifactDestination, ArtifactRun};
pub use document_record::{DocumentRecord, MethodsParagraph, Report, ReportExt};
pub use input_evidence::{
    AffectedAnalysis, BackgroundWait, EvidenceConflict, InspectionCode, InspectionIssue,
    InspectionRelevance, Mutation, SavePolicy, SubmittedInput, check, check_in, conflicts,
    conflicts_in, link_generated_input, lock, preserve_input, read_input,
    reference_input_with_dependencies, register, reindex, relative, relink, resolve, stamp,
    verified_source, verify_ancestors, wait_in_background, write_json,
};
pub use jobs::{job_index_path, jobs};
pub use links::{AlignmentLink, AlignmentLinkExt, metadata_present, sidecar, view_sidecar};
pub use provenance::{
    bundle_for_result, companion, export_file, file_run_entry, inspect, package, publish,
    publish_run, read_record, retained_file, verify, verify_export, write_reports,
};
pub use replay::{
    PublishedWithFailure, RecoveryCompletion, RecoveryMode, RecoveryPhase, ReplayReport,
    prepare_recovery, recover, recover_verified_prefix, recovery_sessions, replay,
};
