//! The JSON Schema of every record and request basecraft writes or reads, one
//! line per schema file. `basecraft schemas DIR` writes them; `just
//! schemas-check` compares them with the committed `schemas/` folder.
use std::path::Path;

use schemars::Schema;
use serde_json::json;

use crate::{
    companion_records, delimitation, distance_records, export_records, input_records,
    installation_records, link_records, metadata, metadata_records, operation, plugin,
    producer_records, provenance, publication_records, query_records, records, report_records,
    sanderson_records, sanger, sanger_records, submission, submission_records,
};

macro_rules! schemas {
    ($($name:literal => $type:ty,)*) => {
        /// Every schema file's name (without `.schema.json`) and schema.
        pub fn all() -> Vec<(&'static str, Schema)> {
            vec![$(($name, schemars::schema_for!($type)),)*]
        }
    };
}

#[rustfmt::skip]
schemas! {
    "sanger-saved-review" => sanger_records::SavedReview,
    "sanger-saved-call-sets" => sanger_records::SavedCallSets,
    "sanger-review-evidence" => sanger_records::ReviewEvidence,
    "sanger-review-source" => sanger_records::ReviewSource,
    "sanger-review-support" => sanger_records::ReviewSupport,
    "sanger-review-choice" => sanger::ReviewChoice,
    "sanger-mask-settings" => sanger::MaskSettings,
    "sanger-review-op" => sanger::ReviewOp,
    "sanger-review-settings" => sanger::ReviewSettings,
    "sanger-review-reference" => sanger::ReviewReference,
    "sanger-draft" => sanger_records::SangerDraft,
    "sanger-placement" => sanger_records::SangerPlacement,
    "sanger-request" => sanger_records::SangerRequest,
    "alignment-link" => records::AlignmentLink,
    "alignment-link-recovery" => link_records::AlignmentLinkRecovery,
    "alignment-link-unavailable" => link_records::AlignmentLinkUnavailable,
    "collection-export-intent" => publication_records::CollectionExportIntent,
    "member-export-receipt" => publication_records::MemberExportReceipt,
    "report-export-manifest" => report_records::ReportExportManifest,
    "report-source-map" => Vec<report_records::ReportSourceMapEntry>,
    "report-citations" => Vec<String>,
    "document-record" => report_records::DocumentRecord,
    "document-report" => report_records::Report,
    "metadata-manifest" => metadata_records::MetadataManifest,
    "project-metadata" => metadata::ProjectMetadata,
    "metadata-edit" => metadata::MetadataEdit,
    "query-preparation" => query_records::QueryPlan,
    "query-source" => Vec<query_records::Query>,
    "validated-result" => records::ValidatedResult,
    "workflow-stages" => Vec<operation::OperationOutcome>,
    "workflow-failure" => records::WorkflowFailure,
    "submission" => submission::Submission,
    "submission-edit" => submission::SubmissionEdit,
    "submission-package" => submission_records::SubmissionPackage,
    "submission-validation" => submission_records::ValidationSummary,
    "submitted-input" => input_records::RunInput,
    "activity-input-run" => records::ActivityInputRun,
    "activity-contributions" => records::ActivityContributions,
    "extraction-record" => records::ExtractionRecord,
    "journal-entry" => records::JournalEntry,
    "companion" => companion_records::Companion,
    "feature" => crate::Feature,
    "sequence-arrow-pair" => plugin::ArrowPair,
    "sanderson-report" => sanderson_records::SandersonReport,
    "haplotypes" => producer_records::Haplotypes,
    "delimitation-progress" => producer_records::DelimitationProgress,
    "delimitation-report" => producer_records::DelimitationReport,
    "mptp-failure" => producer_records::MptpFailure,
    "blast-receipt" => producer_records::BlastReceipt,
    "mafft-addition" => producer_records::MafftAddition,
    "primer-report" => producer_records::PrimerReport,
    "zenodo-selection" => producer_records::ZenodoSelection,
    "zenodo-receipt" => producer_records::ZenodoReceipt,
    "iqtree-recovery" => producer_records::IqtreeRecovery,
    "distance-matrix" => distance_records::DistanceMatrix,
    "identified-distance-matrix" => distance_records::IdentifiedMatrix,
    "distance-cache-header" => distance_records::CacheHeader,
    "distance-bootstrap" => distance_records::BootstrapReport,
    "distance-run-report" => distance_records::DistanceRunReport,
    "distance-group-report" => distance_records::DistanceGroupReport,
    "delimitation-request" => delimitation::DelimitationRequest,
    "delimitation-result" => delimitation::DelimitationResult,
    "delimitation-workspace" => delimitation::DelimitationWorkspace,
    "analysis-run" => records::AnalysisRun,
    "analysis-index" => records::AnalysisIndex,
    "run-record" => records::RunRecord,
    "job-record" => records::JobRecord,
    "store-marker" => records::StoreMarker,
    "indexed-folders" => records::IndexedFolders,
    "pending-save" => records::PendingSave,
    "recovery-integrity-error" => records::RecoveryIntegrityError,
    "result-binding" => records::ResultBinding,
    "provenance-export-map" => export_records::ProvenanceExportMap,
    "provenance-publication" => provenance::Publication,
    "provenance-latest" => provenance::LatestRecord,
    "operation-contract" => operation::OperationContract,
    "operation-request" => operation::OperationRequest,
    "operation-outcome" => operation::OperationOutcome,
    "operation-failure" => operation::OperationFailure,
    "plugin-invoke-result" => plugin::InvokeResult,
    "plugin-installation-index" => installation_records::InstallationIndex,
    "managed-plugin-runtime" => installation_records::Runtime,
}

/// Write every schema as `DIR/<name>.schema.json`. A record written through
/// [`records::encode`] names the basecraft version that wrote it under
/// `basecraft`, so its schema requires that field.
pub fn write_all(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, mut schema) in all() {
        let stamped = schema
            .get("title")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|title| records::STAMPED.contains(&title));
        if stamped {
            let fields = schema.ensure_object();
            let properties = fields.entry("properties").or_insert_with(|| json!({}));
            properties[records::WRITTEN_BY] = json!({
                "description": "The basecraft version that wrote this file.",
                "type": "string"
            });
            let required = fields.entry("required").or_insert_with(|| json!([]));
            if let Some(required) = required.as_array_mut() {
                required.push(records::WRITTEN_BY.into());
            }
        }
        let mut text = serde_json::to_string_pretty(&schema)?;
        text.push('\n');
        std::fs::write(dir.join(format!("{name}.schema.json")), text)?;
    }
    Ok(())
}
