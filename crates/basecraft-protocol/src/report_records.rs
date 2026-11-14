//! The files a report export writes, and the document record they describe.
//!
//! Writers convert objects through `serde_json::Value` to keep keys sorted at
//! every depth.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// One paragraph's activity references in a report's source map.
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
    #[schemars(with = "serde_json::Value")]
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
