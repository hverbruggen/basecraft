//! The plugin protocol: JSON-RPC 2.0, one message per line over a Unix
//! socket. Bulk data travels as sealed shared-memory buffers attached to
//! the message as file descriptors; a message's `fds` field says how many
//! it carries, and `buffer` fields index into them.
//!
//! Plugins receive immutable snapshots and return operations. They never
//! change documents themselves.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Op;

pub const PROTOCOL_VERSION: u32 = 1;

/// A request or a response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    /// Number of file descriptors attached to this message.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fds: usize,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl Message {
    pub fn request(id: u64, method: &str, params: Value, fds: usize) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
            fds,
        }
    }
}

/// `initialize` result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PluginInfo {
    pub protocol: u32,
    pub id: String,
    pub version: String,
    pub operations: Vec<String>,
}

/// A sequence snapshot: two Arrow IPC streams in shared memory.
///
/// `residues`: one record batch, one non-null `uint8` column `residues`.
/// `features`: one row per segment, columns `feature` (u32 id), `kind`
/// (utf8), `start`, `end` (u64, 0-based half-open), `strand` (i8: 1, -1, 0)
/// and `label` (utf8, nullable).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SequenceSnapshot {
    pub doc: String,
    pub name: String,
    pub version: u64,
    pub len: u64,
    pub circular: bool,
    /// Index of the residues buffer among the message's descriptors.
    pub residues: usize,
    /// Index of the features buffer among the message's descriptors.
    pub features: usize,
}

/// `invoke` parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Invoke {
    pub operation: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SequenceSnapshot>,
}

/// `invoke` result: proposed operations against the snapshot's version,
/// and for operations that write files, what they wrote and with which tool.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InvokeResult {
    #[serde(default)]
    pub ops: Vec<Op>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Paths of the files the operation wrote, as given in its parameters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    /// The external program that did the work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolInfo>,
    /// Actual process evidence supplied by newer plugins; absence stays unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionInfo {
    pub argv: Vec<String>,
    #[serde(default)]
    pub resolved_parameters: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub preprocessing: Vec<String>,
    /// Only scientific environment settings, never the whole process environment.
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub methods_template: Option<String>,
    /// Frozen scientific description and citations supplied by the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub methods: Option<Box<MethodsInfo>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MethodsInfo {
    pub description: String,
    #[serde(default)]
    pub citations: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}
