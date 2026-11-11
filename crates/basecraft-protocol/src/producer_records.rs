//! Records emitted by the bundled analysis adapters.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct MafftAddition {
    pub rows: Vec<(String, String)>,
    pub reference: Vec<String>,
    pub insertions: usize,
    pub reversed: Vec<String>,
    pub parameters: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PrimerReport {
    pub keep_start: usize,
    pub keep_end: usize,
    pub orientation: String,
    pub hits: Vec<PrimerHit>,
    pub forward: String,
    pub reverse: String,
    pub minimum_overlap: usize,
    pub error_percent: u32,
    pub window: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PrimerHit {
    pub side: String,
    pub start: usize,
    pub end: usize,
    pub overlap: usize,
    pub errors: usize,
    pub informative_matches: usize,
    pub evidence_score: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ZenodoSelection {
    pub schema: u32,
    pub selection: String,
    pub files: Vec<ZenodoSnapshot>,
    pub total_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ZenodoSnapshot {
    pub name: String,
    pub snapshot: PathBuf,
    pub size: u64,
    pub sha256: String,
    pub md5: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ZenodoFile {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub md5: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ZenodoMetadata {
    pub title: String,
    pub description: String,
    pub creators: Vec<ZenodoCreator>,
    pub upload_type: String,
    pub access_right: String,
    pub license: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ZenodoCreator {
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ZenodoReceipt {
    pub schema: u32,
    pub status: ZenodoStatus,
    pub service: String,
    pub selection: String,
    pub metadata: ZenodoMetadata,
    pub draft_id: Option<u64>,
    pub url: String,
    pub files: Vec<ZenodoFile>,
    pub uploaded: Vec<String>,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ZenodoStatus {
    DraftReady,
    CreationUnknown,
    UploadFailed,
    Cancelled,
    Uploading,
    #[serde(untagged)]
    Unknown(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IqtreeRecovery {
    pub version: u32,
    pub artifacts: Vec<IqtreeRecoveryArtifact>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct IqtreeRecoveryArtifact {
    pub destination: PathBuf,
    pub staged: Option<PathBuf>,
    pub previous: Option<PathBuf>,
}
