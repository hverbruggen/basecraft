//! Basecraft's own records about documents and analyses: provenance
//! publications and bundles, alignment links, document History and Methods,
//! journal replay and recovery, analysis runs and the evidence of their
//! inputs, and save guards. Other crates use the names exported here.

pub(crate) mod analysis_store;
pub(crate) mod contributions;
pub(crate) mod document_record;
pub(crate) mod jobs;
pub(crate) mod links;
pub(crate) mod provenance;
pub(crate) mod publish;
pub(crate) mod replay;
pub(crate) mod run_inputs;
pub(crate) mod save_guard;

pub use analysis_store::ArtifactRun;
pub use document_record::{DocumentRecord, MethodsParagraph, Report, ReportExt};
pub use jobs::jobs;
pub use links::{
    AlignmentLink, AlignmentLinkExt, metadata_present, sidecar, sidecar_in, view_sidecar,
    view_sidecar_in,
};
pub use provenance::{
    HistoryGap, SavedHistory, bundle_for_result, companion, file_run_entry, inspect, latest,
    latest_record, move_file, move_into_project, package, publish, publish_run, read_record,
    retained_file, saved_history, verify, verify_export,
};
pub use replay::{
    PublishedWithFailure, RecoveryCompletion, RecoveryMode, RecoveryPhase, ReplayReport,
    prepare_recovery, recover, recover_verified_prefix, recovery_sessions, replay,
};
pub use run_inputs::{
    AffectedAnalysis, BackgroundWait, EvidenceConflict, InspectionCode, InspectionIssue,
    InspectionRelevance, Locks, SavePolicy, SubmittedInput, check, check_in, conflicts,
    conflicts_in, link_generated_input, lock, lock_in, preserve_input, read_input, reference_input,
    register, reindex, relative, relink, resolve, stamp, verified_source, verify_ancestors,
    wait_in_background, write_input, write_json, write_plain_json,
};
