//! One registry of current record examples, using the production wire types.
use basecraft_protocol::{
    export_records::*, input_records::*, installation_records::*, link_records::*,
    metadata_records::*, publication_records::*, query_records::*, records::*, report_records::*,
    sanger_records::*, submission_records::*,
};
use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};

type Encoder = fn(&[u8], bool) -> Vec<u8>;
fn encode<T: DeserializeOwned + Serialize>(original: &[u8], sorted: bool) -> Vec<u8> {
    // A record written through `records::encode` names its writer; the
    // record type does not hold that field.
    let mut value: serde_json::Value = serde_json::from_slice(original).unwrap();
    if original.starts_with(b"{\n")
        && let Some(writer) = value.as_object_mut().and_then(|o| o.remove(WRITTEN_BY))
    {
        let record: T = serde_json::from_value(value).unwrap();
        let mut value = serde_json::to_value(record).unwrap();
        value[WRITTEN_BY] = writer;
        let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
        bytes.push(b'\n');
        return bytes;
    }
    let record: T = serde_json::from_slice(original).unwrap();
    let pretty =
        original.contains(&b'\n') && original.starts_with(b"{\n") || original.starts_with(b"[\n");
    let mut bytes = match (sorted, pretty) {
        (true, true) => serde_json::to_vec_pretty(&serde_json::to_value(record).unwrap()),
        (true, false) => serde_json::to_vec(&serde_json::to_value(record).unwrap()),
        (false, true) => serde_json::to_vec_pretty(&record),
        (false, false) => serde_json::to_vec(&record),
    }
    .unwrap();
    if original.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    bytes
}

#[test]
fn current_records_round_trip() {
    // Specific prefixes precede their wider families. The bool selects writers
    // that serialize via Value (recursive key sorting) instead of direct serde.
    let formats: &[(&str, Encoder, bool)] = &[
        ("submitted-input-", encode::<SubmittedInput>, false),
        ("metadata-manifest", encode::<MetadataManifest>, false),
        ("query-preparation", encode::<QueryPlan>, false),
        ("document-record", encode::<DocumentRecord>, false),
        ("document-report", encode::<Report>, false),
        ("submission-package", encode::<SubmissionPackage>, true),
        ("submission-validation", encode::<ValidationSummary>, false),
        ("sanger-saved-review", encode::<SavedReview>, true),
        ("sanger-saved-call-sets", encode::<SavedCallSets>, true),
        ("sanger-request", encode::<SangerRequest>, true),
        ("sanger-execution", encode::<SangerJobRecord>, true),
        ("journal-entry", encode::<JournalEntry>, true),
        ("activity-input-run", encode::<ActivityInputRun>, true),
        (
            "activity-contributions",
            encode::<ActivityContributions>,
            true,
        ),
        ("extraction", encode::<ExtractionRecord>, true),
        ("workflow-failure", encode::<WorkflowFailure>, true),
        (
            "workflow-stages",
            encode::<Vec<basecraft_protocol::operation::OperationOutcome>>,
            false,
        ),
        (
            "validated-artifacts",
            encode::<Vec<(String, PathBuf, String)>>,
            false,
        ),
        ("pending-save", encode::<PendingSave>, true),
        ("analysis-index", encode::<AnalysisIndex>, false),
        ("indexed-folders", encode::<IndexedFolders>, false),
        ("store", encode::<StoreMarker>, true),
        ("execution-", encode::<RunRecord>, true),
        ("job-index-", encode::<LocalJobIndex>, true),
        ("integrity-", encode::<RecoveryIntegrityError>, true),
        ("binding", encode::<ResultBinding>, true),
        ("run-", encode::<AnalysisRun>, true),
        ("collection-export", encode::<CollectionExportIntent>, true),
        ("collection-intent", encode::<CollectionIntent>, true),
        ("member-export", encode::<MemberExportReceipt>, true),
        ("link-repair", encode::<AlignmentLinkRecovery>, true),
        ("link-guard", encode::<AlignmentLinkUnavailable>, true),
        ("link-", encode::<AlignmentLink>, false),
        ("provenance-export-map", encode::<ProvenanceExportMap>, true),
        ("installation-index", encode::<InstallationIndex>, false),
        ("managed-plugin-runtime", encode::<Runtime>, false),
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/records-v1");
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if !matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("json" | "jsonl")
        ) || name == "document-fingerprints.jsonl"
        {
            continue;
        }
        let (encoder, sorted): (Encoder, bool) = if name.ends_with("-source-map.json") {
            (encode::<Vec<ReportSourceMapEntry>>, true)
        } else if name.ends_with("-citations.json") {
            (encode::<Vec<String>>, true)
        } else if name.starts_with("export-report-") {
            (encode::<ReportExportManifest>, true)
        } else if name.starts_with("provenance-report-") {
            (encode::<ProvenanceReportManifest>, true)
        } else {
            let (_, encoder, sorted) = formats
                .iter()
                .find(|(prefix, _, _)| name.starts_with(prefix))
                .unwrap_or_else(|| panic!("no protocol type registered for {name}"));
            (*encoder, *sorted)
        };
        let original = std::fs::read(&path).unwrap();
        eprintln!("checking {name}");
        let current = encoder(&original, sorted);
        if std::env::var_os("BASECRAFT_UPDATE_RECORDS").is_some() {
            std::fs::write(&path, current).unwrap();
        } else {
            assert_eq!(current, original, "{name}");
        }
    }
}
