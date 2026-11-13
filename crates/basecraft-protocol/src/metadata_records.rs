//! Existing metadata manifest and table references.
use crate::metadata::{Column, ImportSettings};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TableEntry {
    pub file: String,
    pub hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Tables {
    pub specimens: TableEntry,
    pub taxa: TableEntry,
    pub identifications: TableEntry,
    pub records: TableEntry,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct MetadataManifest {
    pub format: String,
    pub version: u32,
    pub tables: Tables,
    #[serde(default)]
    pub import: ImportSettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partitions: Vec<crate::delimitation::Hypothesis>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl crate::records::RecordFormat for MetadataManifest {
    fn is_current(&self) -> bool {
        self.version == crate::metadata::METADATA_VERSION
    }
}
