//! Retained Sanderson adapter report, independent of the GPL native solver.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonReport {
    pub schema: u32,
    pub method: String,
    pub observation_convention: String,
    pub source_sha256: String,
    pub source_newick: String,
    pub received_parameters: serde_json::Value,
    pub effective_settings: SandersonSettings,
    pub native_engine: SandersonEngine,
    pub result: SandersonResult,
    pub nodes: Vec<SandersonNode>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonSettings {
    pub branch_units: String,
    pub sites: u64,
    pub root_age: serde_json::Number,
    pub time_unit: String,
    pub penalty: SandersonPenalty,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lambda: Option<serde_json::Number>,
    pub starts: usize,
    pub seed: u64,
    pub constraints: Vec<SandersonConstraint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_validation: Option<SandersonCrossValidation>,
    pub max_iterations: u64,
    pub max_seconds: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonConstraint {
    pub node: usize,
    pub min_age: serde_json::Number,
    pub max_age: serde_json::Number,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonCrossValidation {
    pub log10_start: serde_json::Number,
    pub log10_step: serde_json::Number,
    pub count: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonEngine {
    pub name: String,
    pub version: String,
    pub binary_sha256: String,
    pub optimizer: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonNode {
    pub id: String,
    pub label: String,
    pub original_length_text: Option<String>,
    pub age: f64,
    pub fitted_rate_per_site: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonResult {
    pub ages: Vec<f64>,
    pub rates_per_site: Vec<Option<f64>>,
    pub objective: f64,
    pub gradient_norm: f64,
    pub lambda: f64,
    pub penalty: SandersonPenalty,
    pub sites: f64,
    pub root_age: f64,
    pub optimizer: String,
    pub starts: Vec<SandersonStart>,
    pub cross_validation: Option<Vec<SandersonCvScore>>,
    pub observation_convention: String,
    pub root_penalty: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandersonPenalty {
    Add,
    Log,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonStart {
    pub start: usize,
    pub objective: Option<f64>,
    pub gradient_norm: Option<f64>,
    pub gradient_tolerance: Option<f64>,
    pub iterations: Option<u64>,
    pub termination: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandersonCvScore {
    pub log10_lambda: f64,
    pub lambda: f64,
    pub raw_squared_error: f64,
    pub chi_square_error: f64,
    pub predictions: Vec<f64>,
}
