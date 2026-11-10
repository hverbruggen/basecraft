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
