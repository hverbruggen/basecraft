//! Retained remote-query preparation records.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct QueryPlan {
    pub schema: u32,
    pub alphabet: String,
    pub program: String,
    pub database: String,
    pub original_residues: usize,
    pub limit: usize,
    pub submitted_residues: usize,
    pub oversized: bool,
    pub records: Vec<QueryRecord>,
    /// Omitted from the receipt copy, which retains the query hash.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fasta: String,
    pub query_hash: String,
    pub approval_hash: String,
    pub truncation: bool,
}

/// The sorted field order retains the original Value-based writer bytes.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct QueryRecord {
    pub id: String,
    pub name: String,
    pub omitted: bool,
    pub original_length: usize,
    pub removed_gaps_or_whitespace: usize,
    pub shortened: bool,
    pub source: Value,
    pub submitted_input_ranges: Vec<(u64, u64)>,
    pub submitted_length: usize,
    pub submitted_query_range: [usize; 2],
    pub submitted_source_ranges: Vec<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Query {
    pub name: String,
    pub sequence: String,
    #[serde(default)]
    pub source: Value,
}
