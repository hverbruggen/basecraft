//! Existing submission export and validation records.
use crate::submission::{BatchSettings, ExportSettings, SourceAlignment};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How serious a finding is. Blocking issues keep a package from being
/// ready; review issues need a look but don't block; unavailable means a
/// check could not run, which says nothing about the sequences.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocking,
    Review,
    Info,
    Unavailable,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocking => "blocking",
            Self::Review => "review",
            Self::Info => "info",
            Self::Unavailable => "unavailable",
        }
    }
}

/// A finding, tied to the record (and specimen) it concerns when it
/// concerns one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Issue {
    pub severity: Severity,
    /// `basecraft` for local checks; `table2asn` for its reports.
    pub source: String,
    /// A stable code: Basecraft's, or the tool's (`SEQ_INST.TerminalNs`).
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specimen: Option<String>,
    /// The tool's own severity word, when it reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_severity: Option<String>,
}

impl Issue {
    pub fn local(severity: Severity, code: &str, message: impl Into<String>) -> Self {
        Self {
            severity,
            source: "basecraft".into(),
            code: code.into(),
            message: message.into(),
            record: None,
            specimen: None,
            tool_severity: None,
        }
    }
}

/// What an export wrote.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PackageSummary {
    pub directory: String,
    /// Hash of the four table2asn input files together; a validation run
    /// belongs to exactly this hash.
    pub input_hash: String,
    pub records: usize,
    pub blocking: usize,
    pub review: usize,
    /// `draft` (blocking issues remain), `prepared` (no local blocking
    /// issues; not yet validated), `validated` (table2asn reported no
    /// errors for these exact files) or `validation_errors`.
    pub status: String,
    pub files: BTreeMap<String, String>,
}

/// A table2asn run, as recorded in the package.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationSummary {
    pub input_hash: String,
    /// `passed` (completed, a report was read and it has no errors),
    /// `errors`, `unavailable` (the tool could not run: nothing is known
    /// about the sequences) or `failed`.
    pub status: String,
    pub tool_version: String,
    pub run_id: String,
    /// Package-relative folder with the raw reports.
    pub directory: String,
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub message: String,
    /// Hashes of the `.sqn` and `.gbf` copied into `genbank/`.
    #[serde(default)]
    pub copies: BTreeMap<String, String>,
    #[serde(default)]
    pub issues: Vec<Issue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SubmissionPackage {
    pub schema: String,
    pub version: u32,
    pub destination: String,
    pub note: String,
    pub status: String,
    pub submission: PackageDocument,
    pub source: Option<SourceAlignment>,
    pub metadata_manifest_hash: Option<String>,
    pub batch: BatchSettings,
    pub export: ExportSettings,
    pub input_hash: String,
    pub records: Vec<PackageRecord>,
    pub issues: PackageIssues,
    pub files: BTreeMap<String, String>,
    pub validation: Option<ValidationSummary>,
    pub tools: PackageTools,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PackageDocument {
    pub document: String,
    pub hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PackageRecord {
    pub export_id: Option<String>,
    pub record: String,
    pub row: usize,
    pub specimen: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PackageIssues {
    pub blocking: usize,
    pub review: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PackageTools {
    pub table2asn: Option<String>,
}
