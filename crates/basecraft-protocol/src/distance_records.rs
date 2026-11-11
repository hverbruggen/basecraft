//! Distance analysis outputs. Numerical algorithms live in the distance engine.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DistanceModel {
    P,
    Jc69,
    K2p,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DistanceMethod {
    Upgma,
    Sorted,
    Fourpoint,
    Njstar,
    Mvrstar,
}
/// Triangular observed distances, in row order: (1,0), (2,0), (2,1), ... .
/// JSON null denotes a missing value. This cache is never changed by tree inference.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DistanceMatrix<M = DistanceModel> {
    pub schema: u32,
    pub input_hash: String,
    pub names: Vec<String>,
    pub columns: usize,
    pub model: M,
    pub min_overlap: u64,
    pub gap_policy: String,
    #[serde(with = "nullable")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub distances: Vec<f64>,
    pub shared_sites: Vec<u64>,
    #[serde(with = "nullable")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub variances: Vec<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IdentifiedMatrix<T = DistanceMatrix> {
    #[serde(flatten)]
    pub matrix: T,
    pub basecraft_identity: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CacheHeader<M = DistanceModel> {
    pub schema: u32,
    pub input_hash: String,
    pub names: Vec<String>,
    pub columns: usize,
    pub model: M,
    pub min_overlap: u64,
    pub gap_policy: String,
    pub basecraft_identity: serde_json::Value,
}
mod nullable {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[f64], s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(v.len()))?;
        for x in v {
            seq.serialize_element(&x.is_finite().then_some(*x))?;
        }
        seq.end()
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<f64>, D::Error> {
        Ok(Vec::<Option<f64>>::deserialize(d)?
            .into_iter()
            .map(|x| x.unwrap_or(f64::NAN))
            .collect())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TreeResult {
    pub trees: Vec<String>,
    pub components: usize,
    pub unresolved: bool,
    pub details: Vec<serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GroupSummary {
    pub group: String,
    pub specimens: usize,
    pub mean_within: Option<f64>,
    pub max_within: Option<f64>,
    pub min_between: Option<f64>,
    pub nearest_group: Option<String>,
    pub nearest_groups: Vec<String>,
    pub observed_between: usize,
    pub barcode_gap: Option<bool>,
    pub observed_within: usize,
    pub missing_within: usize,
    pub missing_between: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReplicateCollection {
    pub path: PathBuf,
    pub format: String,
    pub status: String,
    pub trees: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BootstrapReport<M = DistanceMethod> {
    pub seed: u64,
    pub requested: usize,
    pub completed: usize,
    pub attempted: usize,
    pub method: M,
    pub support_kind: String,
    pub support_scale: String,
    pub support_withheld: bool,
    pub cancelled: bool,
    pub failures: Vec<String>,
    pub failed_replicates: usize,
    pub distance_s: f64,
    pub tree_s: f64,
    pub total_s: f64,
    pub algorithm: String,
    pub rng: String,
    /// Set only after atomic publication. Progress never advertises scratch files.
    pub replicate_trees: Option<ReplicateCollection>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DistanceCalculation<M = DistanceModel, T = DistanceMethod> {
    pub model: M,
    pub method: T,
    pub min_overlap: u64,
    pub gap_policy: String,
    pub input_hash: String,
    pub cache_reused: bool,
    pub sequences: usize,
    pub columns: usize,
    pub distance_threads: usize,
    pub tree_threads: usize,
    pub distance_s: f64,
    pub total_s: f64,
    pub zero_overlap_pairs: usize,
    pub short_overlap_pairs: usize,
    pub saturated_pairs: usize,
    pub tree: Option<TreeResult>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DistanceRunReport<M = DistanceModel, T = DistanceMethod> {
    pub tool: String,
    pub version: String,
    pub bootstrap: Option<BootstrapReport<T>>,
    #[serde(flatten)]
    pub calculation: Option<DistanceCalculation<M, T>>,
    pub output_status: OutputStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutputStatus {
    Written,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DistanceGroupReport<M = DistanceModel> {
    pub model: M,
    pub min_overlap: u64,
    pub gap_policy: String,
    pub input_hash: String,
    pub unassigned: usize,
    pub groups: Vec<GroupSummary>,
}
