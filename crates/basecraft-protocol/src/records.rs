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

/// The record types written through [`encode`], by schema title: their
/// generated schemas require `basecraft`. A record type added to `encode`
/// joins this list (a test checks the committed examples against it).
pub const STAMPED: &[&str] = &[
    "AlignmentLink",
    "AlignmentLinkRecovery",
    "AlignmentLinkUnavailable",
    "AnalysisIndex",
    "AnalysisRun",
    "CollectionExportIntent",
    "CollectionIntent",
    "Companion",
    "IndexedFolders",
    "InstallationIndex",
    "LocalJobIndex",
    "MemberExportReceipt",
    "MetadataManifest",
    "OperationOutcome",
    "OperationRequest",
    "PendingSave",
    "ProvenanceExportMap",
    "ProvenanceReportManifest",
    "Publication",
    "QueryPlan",
    "RecoveryIntegrityError",
    "Report",
    "ReportExportManifest",
    "ResultBinding",
    "RunRecord",
    "Runtime",
    "SangerJobRecord",
    "SangerRequest",
    "SavedCallSets",
    "SavedReview",
    "StoreMarker",
    "Submission",
    "SubmissionPackage",
    "SubmittedInput",
    "ValidationSummary",
    "WorkflowFailure",
];

thread_local! {
    static WRITING_AS: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The version records written on this thread name: this build's, or the one
/// [`writing_as`] set.
pub fn writing_version() -> String {
    WRITING_AS
        .with(|v| v.borrow().clone())
        .unwrap_or_else(|| crate::VERSION.to_owned())
}

/// Run `f` writing records as `version` wrote them. Replay uses it to
/// reproduce the bytes a journal's saves wrote, whichever build wrote them.
pub fn writing_as<R>(version: &str, f: impl FnOnce() -> R) -> R {
    let previous = WRITING_AS.with(|v| v.borrow_mut().replace(version.to_owned()));
    let result = f();
    WRITING_AS.with(|v| *v.borrow_mut() = previous);
    result
}

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
    fields.insert(WRITTEN_BY.into(), writing_version().into());
    let mut bytes = serde_json::to_vec_pretty(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Reads a record written by [`encode`]. A record that names no writing
/// version, or does not fit `T`, fails with an error naming the version that
/// wrote it and the version reading it.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    decode_written(bytes).map(|(record, _)| record)
}

/// [`decode`], with the version that wrote the record.
pub fn decode_written<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<(T, String)> {
    let (value, writer) = stamped(bytes)?;
    Ok((convert(value, &writer)?, writer))
}

/// A record's fields and the version that wrote it. Refuses bytes that are
/// not JSON and records that name no version (rule 5: no reading of
/// unstamped records).
fn stamped(bytes: &[u8]) -> serde_json::Result<(Value, String)> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|e| {
        serde::de::Error::custom(format!(
            "not JSON; basecraft {} cannot read it: {e}",
            crate::VERSION
        ))
    })?;
    let writer = value.as_object_mut().and_then(|o| o.remove(WRITTEN_BY));
    match writer.as_ref().and_then(Value::as_str) {
        Some(writer) => Ok((value, writer.to_owned())),
        None => Err(serde::de::Error::custom(format!(
            "record names no basecraft version that wrote it; basecraft {} reads only records that do",
            crate::VERSION
        ))),
    }
}

/// `value`, read from a record `writer` wrote, as `T`.
pub fn convert<T: DeserializeOwned>(value: Value, writer: &str) -> serde_json::Result<T> {
    T::deserialize(value).map_err(|e| unreadable(writer, &e))
}

/// The error for a record this build cannot read, naming the version that
/// wrote it and the version reading it.
pub fn unreadable(writer: &str, error: &dyn std::fmt::Display) -> serde_json::Error {
    serde::de::Error::custom(format!(
        "record written by basecraft {writer}; basecraft {} cannot read it: {error}",
        crate::VERSION
    ))
}

/// The refusal for a record in a format version this build doesn't read,
/// naming both basecraft versions.
pub fn other_format(
    writer: &str,
    found: impl std::fmt::Display,
    reads: impl std::fmt::Display,
) -> String {
    format!(
        "record written by basecraft {writer} in format version {found}; basecraft {} reads format version {reads}",
        crate::VERSION
    )
}

/// A record's format: its own format version, where it has one, beside the
/// basecraft version every record names.
pub trait RecordFormat: Serialize + DeserializeOwned + JsonSchema {
    /// Whether this build writes this format version.
    fn is_current(&self) -> bool {
        true
    }
}

/// Fields of a record written by a later basecraft that this build does not
/// know, so writing the record again, or removing it, would drop them (D14).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnknownFields {
    /// The basecraft version that wrote the record.
    pub writer: String,
    /// Each field as a path, `tree.style.colour`; `[]` stands for any element.
    pub fields: Vec<String>,
}

/// What `T`, as this build knows it, would drop from `bytes` on writing them
/// again: only for a record a later basecraft wrote. A field counts when this
/// build's schema of `T` does not name it. A record `T` can't read, or one in
/// another format version, counts as a whole.
pub fn unknown_fields<T: RecordFormat>(bytes: &[u8]) -> Option<UnknownFields> {
    let (value, writer) = stamped(bytes).ok().filter(|(_, w)| is_later(w))?;
    let fields = match T::deserialize(&value) {
        Ok(record) if record.is_current() => {
            let schema = serde_json::to_value(schemars::schema_for!(T)).ok()?;
            let mut fields = std::collections::BTreeSet::new();
            unknown(&value, &schema, &schema, "", &mut fields);
            fields.into_iter().collect()
        }
        _ => vec!["the whole file, in a format this build can't read".to_owned()],
    };
    (!fields.is_empty()).then_some(UnknownFields { writer, fields })
}

/// What removing a record a later basecraft wrote would drop: every field
/// that holds something.
pub fn removed_fields(bytes: &[u8]) -> Option<UnknownFields> {
    let (value, writer) = stamped(bytes).ok().filter(|(_, w)| is_later(w))?;
    let fields: Vec<String> = value
        .as_object()?
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, _)| k.clone())
        .collect();
    (!fields.is_empty()).then_some(UnknownFields { writer, fields })
}

/// Walk `value` along its schema `node`, collecting the keys the schema does
/// not name. `root` holds the definitions `$ref`s point to.
fn unknown(
    value: &Value,
    node: &Value,
    root: &Value,
    at: &str,
    out: &mut std::collections::BTreeSet<String>,
) {
    let mut branches = Vec::new();
    alternatives(node, root, &mut branches);
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                let named = branches.iter().find_map(|b| b.get("properties")?.get(key));
                let open = branches
                    .iter()
                    .find_map(|b| b.get("additionalProperties").filter(|a| **a != false));
                match named.or(open) {
                    Some(sub) => unknown(value, sub, root, &path, out),
                    None => {
                        out.insert(path);
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(item) = branches.iter().find_map(|b| b.get("items")) {
                for value in items {
                    unknown(value, item, root, &format!("{at}[]"), out);
                }
            }
        }
        _ => {}
    }
}

/// `node` and every schema it may stand for: its `$ref` and its `allOf`,
/// `anyOf` and `oneOf` branches, followed all the way down.
fn alternatives<'a>(node: &'a Value, root: &'a Value, out: &mut Vec<&'a Value>) {
    let node = match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => reference
            .strip_prefix("#/")
            .and_then(|path| path.split('/').try_fold(root, |at, key| at.get(key)))
            .unwrap_or(node),
        None => node,
    };
    out.push(node);
    for combinator in ["allOf", "anyOf", "oneOf"] {
        for branch in node
            .get(combinator)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            alternatives(branch, root, out);
        }
    }
}

/// Whether `writer` is a later basecraft than this build.
pub fn is_later(writer: &str) -> bool {
    later(writer, crate::VERSION)
}

/// Whether `version` comes after `than` by semantic version precedence.
/// Build metadata counts only between two nightly builds of one version:
/// the one with the later date (`+yyyymmdd.commit`) is later.
pub fn later(version: &str, than: &str) -> bool {
    let (Some(version_key), Some(than_key)) = (precedence(version), precedence(than)) else {
        return false;
    };
    if version_key != than_key {
        return version_key > than_key;
    }
    let date = |v: &str| {
        let date = v.split_once('+')?.1.split('.').next()?;
        (date.len() == 8).then(|| date.parse::<u32>().ok())?
    };
    date(version).zip(date(than)).is_some_and(|(v, t)| v > t)
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

/// Lightweight `.basecraft/jobs/<id>/manifest.json` of the run's home (its
/// project, or the folder that holds the run). Every path is relative to that
/// home and `/`-separated. The pointed-to `OperationOutcome` in runtime.json
/// stays authoritative; this is not `JobRecord`.
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

    #[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct Note {
        text: String,
        inner: Inner,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    struct Inner {
        count: u32,
    }

    impl RecordFormat for Note {}

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
        let error = decode::<Note>(b"{broken").unwrap_err().to_string();
        assert!(error.starts_with("not JSON; basecraft"), "{error}");
    }

    #[test]
    fn versions_order_by_semantic_precedence() {
        assert!(later("0.10.0", "0.9.0"));
        assert!(later("1.0.0-alpha.2", "1.0.0-alpha.1"));
        assert!(later("1.0.0-alpha.10", "1.0.0-alpha.9"));
        assert!(later("1.0.0-beta", "1.0.0-alpha.9"));
        assert!(later("1.0.0", "1.0.0-beta.3"));
        assert!(!later("0.5.0+20261114.abcdef1", "0.5.0"));
        assert!(later("0.5.0+20261120.abcdef1", "0.5.0+20261113.1234567"));
        assert!(!later("0.5.0+20261113.abcdef1", "0.5.0+20261120.1234567"));
        assert!(!later("0.5.0+20261120.abcdef1", "0.6.0+20261101.1234567"));
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
                fields: vec!["inner.colour".into(), "inner.empty".into(), "later".into()],
            })
        );
        let known = br#"{"basecraft": "99.0.0", "text": "a", "inner": {"count": 2}}"#;
        assert_eq!(unknown_fields::<Note>(known), None);
        let older = br#"{"basecraft": "0.1.0", "text": "a", "inner": {"count": 2}, "later": 1}"#;
        assert_eq!(unknown_fields::<Note>(older), None);
        // A newer record this build can't read counts as a whole.
        let changed = br#"{"basecraft": "99.0.0", "text": "a", "inner": {"count": "two"}}"#;
        let whole = unknown_fields::<Note>(changed).unwrap().fields;
        assert!(whole[0].starts_with("the whole file"), "{whole:?}");
        assert_eq!(
            removed_fields(known).map(|u| u.fields),
            Some(vec!["inner".into(), "text".into()])
        );
    }
}
