//! Versioned public operation contracts. Runtime validation belongs to the host.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONTRACT_VERSION: u32 = 2;
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<QueryDescriptor>,
}

/// A question the plugin answers without inputs, outputs or a run, such as
/// which external engines are installed. The host refuses undeclared names
/// and checks the answer against `result`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryDescriptor {
    pub name: String,
    pub description: String,
    pub result: Value,
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
    #[serde(default, skip_serializing_if = "Needs::is_empty")]
    pub needs: Needs,
    /// A workflow the host runs itself from the stages in `needs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_workflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<String>,
    /// Services the host runs around a bundled plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<HostServices>,
}

/// What an operation needs from the host besides its inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Needs {
    /// Credential name -> environment variable the plugin reads. A need, never
    /// permission: an installed package needs a grant bound to its digest; a
    /// bundled package receives the credential from the user's environment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub credentials: BTreeMap<String, String>,
    /// Parameter the host fills with the user's contact email when the caller
    /// leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_email: Option<String>,
    /// Workflow stage name -> registered operation id, including major version.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workflow_stages: BTreeMap<String, String>,
    /// Parameters that receive the CPU threads granted to the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cpu_parameters: Vec<String>,
}

impl Needs {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostServices {
    /// The result holds species hypotheses for the target workspace.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub species_workspace: bool,
    /// The operation is a base caller the Sanger review asks for alternative
    /// calls. Its contract answers the `engine` query with the program's name
    /// and SHA-256, and its primary output is the draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<CallerKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteService>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallerKind {
    SingleTrace,
}

/// A bundled service that sends data elsewhere: how the host cancels it,
/// retains its receipt and freezes what it submits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteService {
    pub lifecycle: LifecyclePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_preparation: Option<QueryPreparation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_query_transformation: Option<String>,
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
    Equals {
        parameter: String,
        value: Value,
    },
    OneOf {
        parameter: String,
        values: Vec<Value>,
    },
    Positive {
        parameter: String,
    },
}

impl ParameterCondition {
    pub fn parameter(&self) -> &str {
        match self {
            Self::Equals { parameter, .. }
            | Self::OneOf { parameter, .. }
            | Self::Positive { parameter } => parameter,
        }
    }

    /// Whether the effective parameters meet the condition.
    pub fn holds(&self, params: &Value) -> bool {
        let value = params.get(self.parameter());
        match self {
            Self::Equals { value: wanted, .. } => value == Some(wanted),
            Self::OneOf { values, .. } => value.is_some_and(|value| values.contains(value)),
            Self::Positive { .. } => value.and_then(Value::as_u64).is_some_and(|n| n > 0),
        }
    }
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
    /// Residue alphabets the operation accepts. Empty accepts any. The host
    /// refuses other alignments before launch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alphabets: Vec<Alphabet>,
    /// Check the input's retained evidence before the run, as for a reused
    /// distance cache.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verify: bool,
    /// Preserve original and submitted bytes when results refer to input paths.
    /// Applied by the shared runner equally for GUI, CLI, Python and MCP.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retain: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputBinding {
    pub media_type: String,
    pub min: usize,
    pub max: usize,
    pub visibility: ArtifactVisibility,
    /// A collection is allocated one directory; members remain relative to it.
    #[serde(default)]
    pub collection: bool,
    /// The collection member that holds the main result, such as `tree.nwk`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    /// What the support values on trees in this output mean.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tree_support: Vec<TreeSupportRule>,
    /// Bootstrap trees named `bootstrap-<run>.nex` may be exported beside the
    /// exported primary tree when `bootstrap_destination` is `working`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bootstrap_exports: bool,
    /// Facts about the primary member, for its companion file.
    #[serde(default, skip_serializing_if = "OutputFacts::is_empty")]
    pub facts: OutputFacts,
}

pub use crate::facts::{Alphabet, BranchLengths};

/// Facts an output declares about its primary member. The host writes them,
/// marked as stated, into the member's companion file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputFacts {
    /// What a tree's branch lengths measure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_lengths: Option<BranchLengths>,
    /// The facts hold only for runs whose parameters meet this, such as a
    /// corrected distance model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ParameterCondition>,
}

impl OutputFacts {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
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

/// What the host allocated for one run, sent with every invocation. Paths are
/// relative to `run_root`; an input outside the run folder is absolute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvocationContext {
    pub schema: u32,
    pub operation: String,
    pub run_id: String,
    pub run_root: String,
    pub inputs: BTreeMap<String, Vec<String>>,
    pub outputs: BTreeMap<String, String>,
    pub cpu_threads: usize,
    pub cancel_file: String,
    pub progress_file: String,
    #[serde(default)]
    pub input_evidence: BTreeMap<String, Vec<InvocationInputEvidence>>,
    /// The alphabet of each alignment input: what the user or the file's
    /// companion stated, or else the host's guess from the residues.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub alphabets: BTreeMap<String, Alphabet>,
}

/// Where a submitted input came from. In the invocation context its paths
/// follow the context's rule; in the run record they are relative to the run
/// folder.
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
    RecordNamesRepeat,
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
            Self::RecordNamesRepeat => "record_names_repeat",
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
