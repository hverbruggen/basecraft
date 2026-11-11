//! Versioned public operation contracts. Runtime validation belongs to the host.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONTRACT_VERSION: u32 = 1;
pub const CONTRACT_CAPABILITY: &str = "operation-contract-v1";
pub const MAX_DESCRIPTOR_BYTES: usize = 1024 * 1024;
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_NESTING: usize = 32;
pub const MAX_FDS: usize = 16;
pub const MAX_ARTIFACTS: usize = 1024;
pub const MAX_BINDINGS: usize = 64;
pub const MAX_INPUT_FILES: usize = 1024;
pub const MAX_OPERATIONS: usize = 256;
pub const MAX_VALIDATION_WORK: usize = 8_000_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationContract {
    pub schema: u32,
    pub operations: Vec<OperationDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationDescriptor {
    pub name: String,
    /// Semantic version; breaking behavior/default changes require a new major.
    pub version: String,
    pub implementation: String,
    pub title: String,
    pub description: String,
    pub family: ExecutionFamily,
    pub parameters: Value,
    pub result: Value,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputBinding>,
    #[serde(default)]
    pub outputs: BTreeMap<String, OutputBinding>,
    pub effects: Effects,
    /// Logical credential name -> reserved runtime environment variable. This
    /// declares a need, never permission; host grants bind exact accepted code.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub credentials: BTreeMap<String, String>,
    #[serde(default)]
    pub required_features: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_parameter: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cpu_parameters: Vec<String>,
    /// Explicit migration of a public two-argument SDK handler. Runtime controls
    /// come only from the allocated context; scientific callers cannot set them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_adapter: Option<LegacyAdapter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_workflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<String>,
    /// Host services selected by this contract, available to any accepted plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<HostServices>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostServices {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub species_workspace: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bootstrap_exports: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verify_inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tree_support: Vec<TreeSupportRule>,
    /// Workflow stage name -> registered operation id, including major version.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workflow_operations: BTreeMap<String, String>,
    /// Ambient credentials are supplied only to bundled packages. Installed
    /// packages still need an explicit grant bound to their accepted digests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambient_credentials: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
    /// Program a caller service runs. The host finds it through
    /// `BASECRAFT_<PROGRAM>`, the plugin runtime, then PATH, and pins its hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_engine: Option<String>,
    /// Cooperative cancellation and receipt rules for a bundled remote service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<LifecyclePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_preparation: Option<QueryPreparation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_query_transformation: Option<String>,
    /// Bundled operations may obtain a missing parameter from a named environment setting.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameter_environment: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryPreparation {
    Blast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePolicy {
    ExternalSubmission,
    RetainedReceipt,
    RemoteDraft,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TreeSupportRule {
    pub name: String,
    pub kind: crate::tree::SupportKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ParameterCondition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "test", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterCondition {
    Equals { parameter: String, value: Value },
    Positive { parameter: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegacyAdapter {
    #[serde(default)]
    pub primary_input: Option<String>,
    /// Named input binding -> legacy handler parameter.
    pub inputs: BTreeMap<String, String>,
    #[serde(default)]
    pub input_arrays: Vec<String>,
    /// Legacy output parameter -> allocated role and relative member.
    pub outputs: BTreeMap<String, LegacyOutput>,
    #[serde(default)]
    pub allocation: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegacyOutput {
    pub role: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFamily {
    FileAnalysis,
    SequenceProposal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputView {
    Bytes,
    AlignedFasta,
    UngappedFasta,
    Newick,
    SequenceSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputBinding {
    pub view: InputView,
    pub media_type: String,
    pub min: usize,
    pub max: usize,
    /// Preserve original and submitted bytes when results refer to input paths.
    /// Applied by the shared runner equally for GUI, CLI, Python and MCP.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputBinding {
    pub media_type: String,
    pub min: usize,
    pub max: usize,
    pub visibility: ArtifactVisibility,
    /// A collection is allocated one directory; members remain relative to it.
    #[serde(default)]
    pub collection: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactVisibility {
    Result,
    Supporting,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Effects {
    pub document_write: bool,
    pub artifact_write: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationRequest {
    pub operation: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Vec<InputReference>>,
    pub parameters: Value,
    #[serde(default)]
    pub destinations: BTreeMap<String, String>,
    #[serde(default)]
    pub pins: Option<OperationPins>,
    #[serde(default)]
    pub request_key: Option<String>,
    /// A document that records this run in its history and receives its
    /// results when it is not itself an input, such as a species workspace
    /// analysing a file it references. Needs an owning session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<InputReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputReference {
    File {
        path: String,
    },
    Document {
        doc: String,
        generation: u64,
        revision: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationPins {
    pub generation: String,
    pub package_digest: String,
    pub descriptor_digest: String,
}

/// Host-owned wire context, sent only after capability negotiation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvocationContext {
    pub schema: u32,
    pub operation: String,
    pub run_id: String,
    pub inputs: BTreeMap<String, Vec<String>>,
    pub outputs: BTreeMap<String, String>,
    pub cpu_threads: usize,
    pub cancel_file: String,
    pub progress_file: String,
    #[serde(default)]
    pub input_evidence: BTreeMap<String, Vec<InvocationInputEvidence>>,
    /// Host-owned legacy compatibility metadata, never caller parameters.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub legacy_parameters: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvocationInputEvidence {
    pub original_path: Option<String>,
    pub source_hash: Option<String>,
    pub submitted_hash: String,
    pub transformation: String,
    pub run_directory: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginArtifact {
    pub role: String,
    /// Relative to the allocated output for this role, never a destination.
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Queued,
    Running,
    Cancelling,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Partial,
}

impl RuntimeState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted | Self::Partial
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationErrorCode {
    IoError,
    FormatError,
    UnknownFormat,
    EmptyDocument,
    AnalysisInputConflict,
    EvidenceCheckIncomplete,
    SaveDecisionStale,
    PublishedWithFailure,
    NothingToDo,
    ChangedOnDisk,
    WrongDocumentKind,
    UnknownDocument,
    NotFound,
    Damaged,
    Limit,
    Unsupported,
    Busy,
    InvalidArguments,
    IncompatibleContract,
    PluginDisabled,
    PluginUnavailable,
    PluginChanged,
    PermissionDenied,
    StaleRevision,
    OutputConflict,
    UnsupportedInput,
    PluginProtocolViolation,
    ToolFailure,
    Cancelled,
    Timeout,
    PublicationIncomplete,
    EvidenceIncomplete,
}

impl OperationErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IoError => "io_error",
            Self::FormatError => "format_error",
            Self::UnknownFormat => "unknown_format",
            Self::EmptyDocument => "empty_document",
            Self::AnalysisInputConflict => "analysis_input_conflict",
            Self::EvidenceCheckIncomplete => "evidence_check_incomplete",
            Self::SaveDecisionStale => "save_decision_stale",
            Self::PublishedWithFailure => "published_with_failure",
            Self::NothingToDo => "nothing_to_do",
            Self::ChangedOnDisk => "changed_on_disk",
            Self::WrongDocumentKind => "wrong_document_kind",
            Self::UnknownDocument => "unknown_document",
            Self::NotFound => "not_found",
            Self::Damaged => "damaged",
            Self::Limit => "limit",
            Self::Unsupported => "unsupported",
            Self::Busy => "busy",
            Self::InvalidArguments => "invalid_arguments",
            Self::IncompatibleContract => "incompatible_contract",
            Self::PluginDisabled => "plugin_disabled",
            Self::PluginUnavailable => "plugin_unavailable",
            Self::PluginChanged => "plugin_changed",
            Self::PermissionDenied => "permission_denied",
            Self::StaleRevision => "stale_revision",
            Self::OutputConflict => "output_conflict",
            Self::UnsupportedInput => "unsupported_input",
            Self::PluginProtocolViolation => "plugin_protocol_violation",
            Self::ToolFailure => "tool_failure",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::PublicationIncomplete => "publication_incomplete",
            Self::EvidenceIncomplete => "evidence_incomplete",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OperationError {
    pub code: OperationErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PublishedArtifact {
    pub role: String,
    pub path: String,
    /// Algorithm-qualified digest, e.g. b3:hex; never reinterpret old hashes.
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProposalOutcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    pub applied: usize,
    pub rejected: Vec<String>,
}

/// Structured rejection before a run has an outcome or durable identity.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OperationFailure {
    pub schema: u32,
    pub operation: String,
    pub state: RuntimeState,
    pub errors: Vec<OperationError>,
}

/// Session-owned runtime view, independent of historical scientific `JobStatus`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OperationOutcome {
    pub schema: u32,
    pub operation: String,
    pub run_id: String,
    pub state: RuntimeState,
    pub pins: OperationPins,
    pub requested_parameters: Value,
    pub effective_parameters: Value,
    pub data: Option<Value>,
    pub artifacts: Vec<PublishedArtifact>,
    pub proposals: Option<ProposalOutcome>,
    pub warnings: Vec<String>,
    pub errors: Vec<OperationError>,
    pub evidence: Option<String>,
}
