//! Fresh report-output shapes; existing selected-field ownership readers are separate.
//!
//! Writers convert objects through `serde_json::Value` to retain recursive key
//! ordering. Provenance appends one LF; standalone exports do not. These types
//! describe current outputs, not historical acceptance or lossless rewrites.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Publication report metadata, distinct from a standalone report export.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceReportManifest {
    pub schema_version: u32,
    pub publication: String,
    pub template_version: u32,
    pub output_hashes: Vec<String>,
    pub redactions: Vec<String>,
    pub citations: Vec<String>,
}

/// Standalone report metadata; document and revision retain their exact strings.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReportExportManifest {
    pub schema_version: u32,
    pub revision: String,
    pub document: String,
    pub unsaved: bool,
    pub template_version: u32,
    pub coordinates: String,
}

/// One paragraph's activity references, shared by both unversioned map arrays.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReportSourceMapEntry {
    pub paragraph: usize,
    pub activities: Vec<String>,
}

use crate::{LogEntry, provenance::EditActivity};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DocumentRecord {
    pub schema_version: u32,
    pub document: String,
    pub revision: String,
    pub unsaved: bool,
    pub warnings: Vec<String>,
    pub events: Vec<LogEntry>,
    pub effective: Vec<EditActivity>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct HistoryRow {
    pub id: String,
    pub time: String,
    pub summary: String,
    pub status: String,
    pub contributing: bool,
    pub details: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct MethodsParagraph {
    pub activity_ids: Vec<String>,
    pub text: String,
    pub parameters: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    pub schema_version: u32,
    pub template_version: u32,
    pub document: String,
    pub revision: String,
    pub unsaved: bool,
    pub warnings: Vec<String>,
    pub history: Vec<HistoryRow>,
    pub paragraphs: Vec<MethodsParagraph>,
    pub methods: String,
    pub history_text: String,
    pub raw: String,
    #[serde(default)]
    pub citations: Vec<String>,
}
