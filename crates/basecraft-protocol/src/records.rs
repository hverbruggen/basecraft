//! Recovery, analysis-store and execution records, and the one encoder and
//! decoder every record basecraft writes goes through.
//!
//! A record on disk is indented JSON with keys sorted and a final newline. It
//! carries the version of basecraft that wrote it under `basecraft` (brief
//! rule 6.7); the record types themselves do not hold that field.
use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// The key under which every record names the basecraft version that wrote it.
pub const WRITTEN_BY: &str = "basecraft";

/// The bytes of `record` as basecraft writes it: indented JSON, keys sorted,
/// the writing version under `basecraft`, and a final newline. The record must
/// serialize to a JSON object.
pub fn encode(record: &impl Serialize) -> serde_json::Result<Vec<u8>> {
    let mut value = serde_json::to_value(record)?;
    let Some(fields) = value.as_object_mut() else {
        return Err(serde::ser::Error::custom(
            "a record must be a JSON object to carry the basecraft version",
        ));
    };
    fields.insert(WRITTEN_BY.into(), crate::VERSION.into());
    let mut bytes = serde_json::to_vec_pretty(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Reads a record written by [`encode`]. When the bytes do not fit `T`, the
/// error names the version that wrote them and the version reading them.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|e| unreadable(None, &e))?;
    let writer = value.as_object_mut().and_then(|o| o.remove(WRITTEN_BY));
    T::deserialize(value).map_err(|e| unreadable(writer.as_ref().and_then(Value::as_str), &e))
}

/// Fields of a record written by a later basecraft that this build does not
/// know, so writing the record again would drop them (decision D14).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnknownFields {
    /// The basecraft version that wrote the record.
    pub writer: String,
    /// Each field as a path, `tree.style.colour`; `[]` stands for any element.
    pub fields: Vec<String>,
}

/// What `T`, as this build knows it, would drop from `bytes` on writing
/// them again: only for a record a later basecraft wrote. Fields holding
/// null, false or an empty list or object are not counted, since this build
/// leaves those out of what it writes.
pub fn unknown_fields<T: Serialize + DeserializeOwned>(bytes: &[u8]) -> Option<UnknownFields> {
    let mut value: Value = serde_json::from_slice(bytes).ok()?;
    let writer = value.as_object_mut()?.remove(WRITTEN_BY)?;
    let writer = writer.as_str().filter(|w| is_later(w))?.to_owned();
    let kept = serde_json::to_value(T::deserialize(&value).ok()?).ok()?;
    let mut fields = std::collections::BTreeSet::new();
    dropped(&value, &kept, "", &mut fields);
    (!fields.is_empty()).then(|| UnknownFields {
        writer,
        fields: fields.into_iter().collect(),
    })
}

fn dropped(old: &Value, kept: &Value, at: &str, out: &mut std::collections::BTreeSet<String>) {
    match (old, kept) {
        (Value::Object(old), Value::Object(kept)) => {
            for (key, value) in old {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                match kept.get(key) {
                    Some(kept) => dropped(value, kept, &path, out),
                    None if !is_empty(value) => {
                        out.insert(path);
                    }
                    None => {}
                }
            }
        }
        (Value::Array(old), Value::Array(kept)) => {
            for (value, kept) in old.iter().zip(kept) {
                dropped(value, kept, &format!("{at}[]"), out);
            }
        }
        _ => {}
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    }
}

/// Whether `writer` is a later basecraft than this build.
pub fn is_later(writer: &str) -> bool {
    later(writer, crate::VERSION)
}

/// Whether `version` comes after `than` by semantic version precedence;
/// build metadata does not count. A version that does not parse is not later.
pub fn later(version: &str, than: &str) -> bool {
    precedence(version)
        .zip(precedence(than))
        .is_some_and(|(version, than)| version > than)
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Number(u64),
    Text(String),
}

/// Sort key of a semantic version: the release numbers, then whether it is a
/// release (which follows its pre-releases), then the pre-release parts.
fn precedence(version: &str) -> Option<([u64; 3], bool, Vec<Part>)> {
    let version = version.split('+').next()?;
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let numbers: Vec<u64> = core
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let core: [u64; 3] = numbers.try_into().ok()?;
    let parts = pre.map_or_else(Vec::new, |pre| {
        pre.split('.')
            .map(|p| {
                p.parse()
                    .map_or_else(|_| Part::Text(p.into()), Part::Number)
            })
            .collect()
    });
    Some((core, pre.is_none(), parts))
}

/// The error for a record this build cannot read: which basecraft wrote it
/// (when the record says) and which is reading.
pub fn unreadable(writer: Option<&str>, error: &dyn std::fmt::Display) -> serde_json::Error {
    let reader = crate::VERSION;
    serde::de::Error::custom(match writer {
        Some(writer) => {
            format!(
                "record written by basecraft {writer}; basecraft {reader} cannot read it: {error}"
            )
        }
        None => format!(
            "record names no basecraft version that wrote it; basecraft {reader} cannot read it: {error}"
        ),
    })
}

/// Current pending-save lifecycle label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStatus {
    Pending,
}

/// `pending-save.json`; data and provenance remain separate writes.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PendingSave {
    pub baseline: crate::LogEntry,
    pub schema_version: u32,
    pub session: String,
    pub target: PathBuf,
    /// Always emitted, including JSON null when the target did not exist.
    pub before_hash: Option<String>,
    pub expected_hashes: Vec<String>,
    pub status: RecoveryStatus,
    pub note: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Namespace identity; foreign markers must never be claimed or migrated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum StoreFormat {
    #[serde(rename = "basecraft.analysis-store")]
    AnalysisStore,
}

/// `store.json` in an existing `analyses-v1` namespace. Runtime readers also
/// enforce version 1, ordinary-file safety and the existing 4096-byte limit.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct StoreMarker {
    pub format: StoreFormat,
    pub schema_version: u32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A run folder's `execution.json`: what ran, on what and how. Paths are
/// relative to the run folder, so the folder can move with its project; an
/// input outside the folder has a path that starts with `..`. The request
/// itself, normalised the same way, is the folder's `request.json`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunRecord {
    pub schema: u32,
    pub operation: String,
    pub package_version: String,
    /// The plugin package and its operation descriptor, by content. Null for
    /// basecraft's own tools, such as a Sanger stage.
    pub package_digest: Option<String>,
    pub descriptor_digest: Option<String>,
    /// Where the plugin ran.
    pub environment: RunEnvironment,
    /// Caller label, such as `gui` or `mcp`.
    pub initiator: String,
    pub executor: String,
    pub process_instance: String,
    /// Submitted inputs by binding.
    pub inputs: BTreeMap<String, Vec<crate::operation::InvocationInputEvidence>>,
    pub effective_parameters: Value,
    pub cpu_threads: usize,
    pub stdout: String,
    pub stderr: String,
    pub tool: Option<crate::plugin::ToolInfo>,
    pub execution: Option<crate::plugin::ExecutionInfo>,
}

pub const RUN_RECORD_SCHEMA: u32 = 3;

impl RunRecord {
    /// A record with nothing run yet: no package digests, environment,
    /// inputs, settings, output or tool. Callers fill what they know.
    pub fn new(operation: String, package_version: String, executor: String) -> Self {
        Self {
            schema: RUN_RECORD_SCHEMA,
            operation,
            package_version,
            package_digest: None,
            descriptor_digest: None,
            environment: RunEnvironment::default(),
            initiator: String::new(),
            executor,
            process_instance: String::new(),
            inputs: BTreeMap::new(),
            effective_parameters: Value::Null,
            cpu_threads: 1,
            stdout: String::new(),
            stderr: String::new(),
            tool: None,
            execution: None,
        }
    }
}

/// What a run needs to be repeated in the same environment. No paths.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunEnvironment {
    /// As the plugin reported it, such as "Python 3.12.4". Null for host
    /// workflows and plugins that run no interpreter.
    pub interpreter: Option<String>,
    /// The managed runtime the plugin ran in; null for the system Python and
    /// for host workflows.
    pub runtime: Option<RunRuntime>,
}

/// A managed plugin runtime: the conda specification it was made from and
/// what was installed, by content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunRuntime {
    pub channels: Vec<String>,
    /// Conda packages and their version constraints.
    pub dependencies: BTreeMap<String, String>,
    pub pypi_dependencies: BTreeMap<String, String>,
    /// Hash of the solved Pixi lock.
    pub lock_digest: String,
    pub manifest_digest: String,
    pub reported_versions: BTreeMap<String, String>,
    pub executable_digests: BTreeMap<String, String>,
}

/// The existing two-field plugin identity in the local index. Neither `Actor`'s
/// tagged representation nor `OperationPins` has this shape.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobIndexPlugin {
    pub id: String,
    pub version: String,
}

/// Existing display path wrapper; not a hashed `InputReference` or a new path rule.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct JobIndexSource {
    pub path: String,
}

/// Lightweight application-state `jobs/<id>/manifest.json`. The pointed-to
/// `OperationOutcome` in runtime.json stays authoritative; this is not `JobRecord`.
/// Existing selected-field readers and owner/receipt checks remain unchanged.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LocalJobIndex {
    pub schema_version: u32,
    pub id: String,
    pub status: crate::operation::RuntimeState,
    pub owner_instance: String,
    pub started: String,
    /// Explicit null for nonterminal states; never omitted by the writer.
    pub ended: Option<String>,
    pub plugin: JobIndexPlugin,
    pub operation: String,
    pub effective_parameters: Value,
    pub initiator: String,
    pub source: JobIndexSource,
    pub outputs: BTreeMap<String, String>,
    /// First runtime error message or explicit null.
    pub diagnostics: Option<String>,
    pub runtime_record: PathBuf,
}

/// Existing rollback-durability poison marker. Recovery refuses its presence,
/// regardless of its contents; this model does not replace that safety check.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RecoveryIntegrityError {
    pub schema_version: u32,
    pub verified_length: u64,
    /// Existing human-readable diagnostic, not a new machine error code.
    pub error: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Existing hidden association written by `ArtifactRun::bind`. Runtime readers
/// retain their selective version, namespace, path and content-hash checks.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResultBinding {
    pub schema_version: u32,
    pub run: String,
    /// UTF-8 relative filename, using the existing writer's path conversion.
    pub result: String,
    pub result_hash: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Labels in `run.json`, distinct from the registered operation runtime lifecycle.
/// Unfamiliar labels survive a typed round-trip without being reinterpreted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisRunStatus {
    Referenced,
    Prepared,
    Saved,
    Completed,
}

/// The existing version-one analysis-store `run.json` family.
/// This describes fresh writer shapes; production readers retain their selective
/// JSON checks. Serialize through `Value` for recursive sorting before `write_json`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisRun {
    pub schema_version: u32,
    pub id: String,
    /// Always emitted, including null for a non-UTF-8 reference anchor filename.
    pub result: Option<String>,
    pub status: AnalysisRunStatus,
    /// Absent until referenced publication; a completed run can have an empty
    /// array. Reuse `ArtifactRevision`'s schema, retaining its JSON snapshot because
    /// the public type rejects unknown extensions on deserialization. Fresh
    /// writers still construct these snapshots from the public `ArtifactRevision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Vec<crate::provenance::ArtifactRevision>>")]
    pub outputs: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Existing tree–alignment association, schema 1 (snapshot) or 2 (reference).
/// Field order and direct pretty serialization are part of the existing wire shape.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AlignmentLink {
    pub schema: u32,
    pub source: PathBuf,
    pub source_hash: String,
    pub snapshot: PathBuf,
    pub snapshot_hash: String,
    pub alignment_version: u64,
    /// False for an explicitly associated imported tree: matching names is not inference provenance.
    pub inferred: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_run: Option<PathBuf>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// One file of the analysis index, `.basecraft/analysis-index/<bucket>/<key>.json`:
/// one analysis store (`.basecraft/analyses-v1`) and the files whose paths fall
/// in this bucket that its runs depend on. A project's index sits in the
/// project's `.basecraft` with project-relative paths; a file outside the
/// writing session's project is indexed in a `.basecraft` beside it by name.
/// `<bucket>` is the first two hex digits of the BLAKE3 hash of the path;
/// `<key>` is a hash of `store`. Paths are `/`-separated and relative to the
/// folder holding `.basecraft`. Readers take the union of every file in a
/// bucket, copies a sync tool made included.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisIndex {
    pub schema_version: u32,
    pub store: String,
    pub files: std::collections::BTreeSet<String>,
}

/// `indexed-folders.json` in an analysis store: the index buckets that list
/// this store, relative to the folder holding the store's `.basecraft`. Lets a
/// lost index be noticed and rebuilt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndexedFolders {
    pub schema_version: u32,
    pub folders: std::collections::BTreeSet<String>,
}

/// Checksummed JSONL envelope. Its flattened entry already carries log version `v`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct JournalEntry<T = crate::LogEntry> {
    /// The basecraft version that wrote this line.
    pub basecraft: String,
    #[serde(rename = "_checksum")]
    pub checksum: String,
    #[serde(rename = "_sequence")]
    pub sequence: u64,
    #[serde(flatten)]
    pub entry: T,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowFailure {
    pub version: u32,
    pub job: WorkflowFailureJob,
    pub workspace: Option<crate::delimitation::DelimitationWorkspace>,
    pub metrics: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkflowFailureJob {
    pub status: crate::records::JobStatus,
    pub diagnostics: Vec<String>,
}

/// Embedded provenance for an activity's exact submitted input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActivityInputRun {
    pub schema_version: u32,
    pub id: String,
    pub hash: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActivityContributions {
    pub schema_version: u32,
    pub edits: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExtractionRecord {
    pub schema_version: u32,
    pub source_revision: String,
    #[schemars(with = "serde_json::Value")]
    pub selection: crate::Op,
    pub coordinates: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Outcome of one analysis job, as reported by its engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobRecord {
    pub id: String,
    pub generation: u64,
    pub status: JobStatus,
    pub method: String,
    pub inputs: BTreeMap<String, String>,
    pub outputs: BTreeMap<String, String>,
    pub diagnostics: Vec<String>,
    pub elapsed_seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Completed,
    Failed,
    Cancelled,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Note {
        text: String,
        inner: Inner,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Inner {
        count: u32,
    }

    fn note() -> Note {
        Note {
            text: "a".into(),
            inner: Inner { count: 2 },
        }
    }

    #[test]
    fn records_carry_the_writing_version_and_read_back_without_it() {
        let bytes = encode(&note()).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with(&format!("{{\n  \"basecraft\": \"{}\",", crate::VERSION)));
        assert!(text.ends_with("}\n"));
        assert_eq!(decode::<Note>(&bytes).unwrap(), note());
        assert!(encode(&[1, 2]).is_err());
    }

    #[test]
    fn an_unreadable_record_names_both_versions() {
        let error = decode::<Note>(br#"{"basecraft": "9.1.0", "text": "a"}"#).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("basecraft 9.1.0"), "{message}");
        assert!(message.contains(&format!("basecraft {} cannot read it", crate::VERSION)));
        assert!(message.contains("inner"), "{message}");
        let error = decode::<Note>(br#"{"text": "a"}"#).unwrap_err().to_string();
        assert!(error.contains("names no basecraft version"), "{error}");
    }

    #[test]
    fn versions_order_by_semantic_precedence() {
        assert!(later("0.10.0", "0.9.0"));
        assert!(later("1.0.0-alpha.2", "1.0.0-alpha.1"));
        assert!(later("1.0.0-alpha.10", "1.0.0-alpha.9"));
        assert!(later("1.0.0-beta", "1.0.0-alpha.9"));
        assert!(later("1.0.0", "1.0.0-beta.3"));
        assert!(!later("0.5.0+20261114.abcdef1", "0.5.0"));
        assert!(!later("0.5.0", "0.5.0"));
        assert!(!later("nightly", "0.5.0"));
    }

    #[test]
    fn only_fields_a_later_version_wrote_and_this_one_drops_are_unknown() {
        let newer = br#"{"basecraft": "99.0.0", "text": "a", "inner": {"count": 2, "colour": "red", "empty": []}, "later": 1}"#;
        assert_eq!(
            unknown_fields::<Note>(newer),
            Some(UnknownFields {
                writer: "99.0.0".into(),
                fields: vec!["inner.colour".into(), "later".into()],
            })
        );
        let known = br#"{"basecraft": "99.0.0", "text": "a", "inner": {"count": 2}}"#;
        assert_eq!(unknown_fields::<Note>(known), None);
        let older = br#"{"basecraft": "0.1.0", "text": "a", "inner": {"count": 2}, "later": 1}"#;
        assert_eq!(unknown_fields::<Note>(older), None);
    }
}
