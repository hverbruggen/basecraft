# Schemas and on-disk record inventory

JSON Schema files are generated from `basecraft-protocol` by `just schemas`.
Do not edit generated JSON by hand. `just ci` regenerates them and checks for drift.
This inventory describes existing writers, including records that do not yet have
protocol types or schemas. It does not establish a compatibility policy.

## Reading the inventory

Locations: **R** = temporary application-state recovery session; **A** = permanent
folder-local `.basecraft/analyses-v1/runs/<id>`; **P** = provenance bundle (beside a
result, in project metadata, or application state according to policy); **J** =
application-state job index; **E** = user-selected export; **C** = application
configuration/cache/state. Filenames are relative to the location in that row.
Identical basenames in different locations are different formats.

Source links identify the owning module; function/type names identify the exact
writer/reader without relying on changing line numbers. `Value` means untyped JSON.
“Kept” means unknown fields survive a deserialize/serialize cycle at the stated
level; it does not mean that a workflow which rebuilds a record merges old fields.
“Dropped” means ordinary serde structs ignore unknown fields when reading and omit
them when writing; “rejected” means `deny_unknown_fields`. None of those behaviors
is changed here outside the selected recovery, store marker, version-one registry, execution and local index types.

## Recovery, journal and provenance

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| `recovery.json`: session, source root, baseline event, retention limit, status | R (legacy sessions: project root) | [session.rs](../crates/basecraft-ops/src/session.rs) `Session::track` | [replay.rs](../crates/basecraft-ops/src/replay.rs) `recorded_source_root`; GUI `provenance_bridge::recovery_sessions` | `schema_version: 1` | protocol `records::RecoveryManifest`; [recovery-manifest.schema.json](recovery-manifest.schema.json) | Kept at top level and throughout opaque baseline; readers inspect selected fields |
| `pending-save.json`: session, target, before hash or null, ordered expected hashes, status, note | R | `Session::save_checked` | save success removes it; `Session` drop/cleanup preserves recovery if present; user/repair tools inspect it | `schema_version: 1` | protocol `records::PendingSave`; [pending-save.schema.json](pending-save.schema.json) | Kept by type; writer creates a fresh intent, does not merge an earlier intent |
| `.basecraft/log/<stream>/<month>-<id>-<segment>.jsonl`: checksummed, sequenced events | R / project | [log.rs](../crates/basecraft-ops/src/log.rs) `LogWriter::append_batch` | `read_all`, `read_readable`, `verified_prefix`, `parse_entry`; replay | `v: 1`; `_checksum`, `_sequence` envelope | protocol `LogEntry` / [log-entry.schema.json](log-entry.schema.json); checksum/sequence envelope untyped | Envelope verified then stripped; unknown event fields dropped by `LogEntry`; future operation variants rejected |
| `recovery-integrity-error.json`: verified length and durability diagnostic | R / project | `LogWriter::append_batch` failed rollback | `replay::recover` refuses poisoned recovery | `schema_version: 1` | `Value`; no schema | Reader checks presence; no rewrite |
| `manifest.json`, `records/<publication-id>.json`: publication, ancestry, artifacts, events, effective edits, completeness | P / A | [provenance.rs](../crates/basecraft-ops/src/provenance.rs) `publish`, `publish_operation`; session publication routines | `inspect`, `verify`, `package`; `document_record::read_origin`, evidence ancestry readers | `schema_version: 1` | protocol `Publication` / [provenance-publication.schema.json](provenance-publication.schema.json) | Rejected at publication and several nested record types; `InputReference.extra` kept |
| `input.json`: source evidence, submitted hash/size, transformation, evidence state, optional ancestry/ledger references | A / P | [input_evidence.rs](../crates/basecraft-ops/src/input_evidence.rs) `reference_input_with_dependencies`, `link_generated_input`, `resolve`, `relink`; session/provenance/runner/Sanger adapters | `read_input`, `conflicts_in`, `verified_source`, packaging/ancestry readers | `schema_version: 1` | ops `SubmittedInput`; reuses protocol `InputReference`, `FileStamp`; no own schema | Kept via `extra` on input/reference; source stamp has no extra map; evidence state still string |
| `run.json`: id, result name, status and sometimes outputs | A | `Session::save_inner`, `publish_referenced_analysis`; `input_evidence` reference/generated helpers; runner `prepare` | `input_evidence::result_name`, `scan_run`; artifact binding | `schema_version: 1` | `Value`; no schema | Selective `Value` reads; constructed afresh; variants differ in optional fields |
| `store.json`: analysis namespace marker | `.basecraft/analyses-v1` | [analysis_store.rs](../crates/basecraft-ops/src/analysis_store.rs) `ArtifactRun::create`, compact JSON plus newline | `ArtifactRun::validate_store` | `format: basecraft.analysis-store`, `schema_version: 1` | protocol `StoreMarker` / [store-marker.schema.json](store-marker.schema.json) | Top-level extras kept by type; validation ignores extras; marker not rewritten |
| Result binding (hash-derived filename): run, result filename/hash | `.basecraft/analyses-v1` binding namespace | `ArtifactRun::bind` | `ArtifactRun::for_result` | `schema_version: 1` | `Value`; no schema | Selective reads; fresh writes |
| `analysis-stores.json`: absolute known store paths | C state | `input_evidence::register` | `store_registry`, `stores`, save conflict scan | `schema_version: 1` | protocol `StoreRegistry` / [store-registry.schema.json](store-registry.schema.json) | Top-level extras kept on registration update; absolute path array/count validated |
| `artifacts/<hash>` with logical name `export-map.json`: old/new publication mapping | E packaged P | `input_evidence::export_history` | retained artifact inspection; no dedicated in-app deserializer | `schema_version: 1` | `Value`; no schema | Generated artifact; original records also retained, no rewrite reader |
| `reports/manifest.json`: publication, template, output hashes, redactions, citations | P / A | `provenance::write_reports` | report consumer / inspection (no dedicated typed in-app reader) | `schema_version: 1`, `template_version: 2` | `Value`; no schema | Generated afresh |
| `reports/source-map.json`: paragraph/activity array | P / A | `provenance::write_reports` | report consumer / inspection | No version field | `Vec<Value>`; no schema | Generated afresh |
| `reports/history.md`, `methods.generated.md`, `parameters.tsv`, bundle `README.md` | P / A | `provenance::write_reports`, `publish` | human/report consumers | Report manifest template version | Markdown / TSV, not JSON records | Not applicable; user-authored `reports/methods.md` is preserved, never regenerated |
| `record.json`: document chronology and effective operations | E report folder | [document_record.rs](../crates/basecraft-ops/src/document_record.rs) `Report::write_files` (`raw`) | `DocumentRecord` consumers / exported evidence inspection | `schema_version: 1` | ops `DocumentRecord`; no generated schema | Dropped by struct; events retain existing LogEntry behavior |
| `report.json`: history, paragraphs, methods, raw record, citations | E report folder | `Report::write_files` | report consumer | `schema_version: 1`, `template_version` | ops `Report`; no schema | Dropped by struct |
| `manifest.json`: report revision/document/unsaved/template/coordinates | E report folder | `Report::write_files` | `check_report_folder` before replacement | `schema_version: 1` | `Value`; no schema | Selected ownership fields read; regenerated |
| `source-map.json`, `citations.json`: paragraph map array / citation string array | E report folder | `Report::write_files` | report consumers | No version fields | `Vec<Value>` / `Vec<String>`; no schemas | Generated afresh |
| `history.md`, `methods.generated.md`, `parameters.tsv` | E report folder | `Report::write_files` | report consumers | Companion manifest template version | Markdown / TSV | Generated afresh; ownership check protects unrelated folders/files |

## Document companions and Sanger payloads

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| `<tree>.basecraft.json`: exchange/tree hashes, supports, annotations, style | Beside tree / A hidden metadata | [files.rs](../crates/basecraft-ops/src/files.rs) `write_tree_sidecar`, `write_tree_sidecar_at`; plugin-host `file_tool::write_support_sidecar` | `files::load_tree` | `schema_version: 1`; missing/0 legacy accepted with restricted semantics; historical writer omits version | ops `TreeSidecar`; protocol support/style payloads; no complete schema | Outer unknown fields dropped; nested style `extra` kept; legacy backup retained before replacement |
| `<species>.basecraft.json`: exchange hash, path base, workspace | Beside SPART / A | `files::write_delimitation_sidecar`, `write_delimitation_sidecar_at`; [delimitation.rs](../crates/basecraft-ops/src/delimitation.rs) artifact writer; session save | `files::load_delimitation` | `version: 1` (legacy relative form) / `2` (path base) | ops `DelimitationSidecar`; protocol `DelimitationWorkspace` / [delimitation-workspace.schema.json](delimitation-workspace.schema.json) covers payload only | Outer fields dropped; payload's own serde rules apply |
| `<tree>.alignment.json`, hidden origin/view link records | Beside tree / A | [links.rs](../crates/basecraft-ops/src/links.rs) `AlignmentLink::create`, `create_referenced`, association/clone/save helpers | `AlignmentLink` loading/verification; `PreparedLinks::prepare` | `schema: 1` snapshot / `2` referenced ledger | ops `AlignmentLink`; no schema | Top-level `extra` kept |
| Retained `*.alignment-recovery.json`: prior/intended origin/view bytes | Beside save target, uniquely retained | `PreparedLinks::publish` | manual repair / diagnostic inspection; no automatic recovery writer | `schema: 1` | `Value`; no schema | Generated snapshot, not rewritten |
| Origin/view unavailable guard: diagnostic and recovery filename | Same alignment sidecar locations during partial save | `PreparedLinks::publish` | link loading refuses unusable association | `schema: 1` | `Value`; no schema | Generated guard, not ordinary AlignmentLink |
| `metadata.basecraft.json`: table names/hashes/columns, import settings, partitions | Project metadata folder | [metadata.rs](../crates/basecraft-ops/src/metadata.rs) `encode`, `write` | `metadata::load` | `format`, `version: 1` | ops `Manifest`, `Tables`, `TableEntry`; protocol `ProjectMetadata` / [project-metadata.schema.json](project-metadata.schema.json) covers model, not disk manifest | Manifest top-level extra kept; nested table wrappers drop extras; unknown TSV columns retained by model |
| `specimens.tsv`, `taxa.tsv`, `identifications.tsv`, `records.tsv` | Same metadata folder | `metadata::encode`, `create`, session save | `metadata::load` | Via metadata manifest | protocol metadata row/column types; standard TSV | Header/column handling preserves extra data; no JSON schema for TSV bytes |
| `*.submission.json` | Project / chosen document path | [submission.rs](../crates/basecraft-ops/src/submission.rs) `encode`, `create`; session save | `submission::load` | `version: 1` | protocol `Submission` / [submission.schema.json](submission.schema.json) | Extra maps on Submission and selected nested types kept; other nested structs have their own rules |
| Saved `*.sanger-review.json` / recovery `review.sanger-review.json` / immutable retained review snapshots | Document / R / A | [sanger_review/storage.rs](../crates/basecraft-ops/src/sanger_review/storage.rs) `write_embedded`, `save_durable`, checkpoint | [sanger_review.rs](../crates/basecraft-ops/src/sanger_review.rs) `decode`, `from_stored`, `load`, `recover` | `StoredReview.version: 1`; `StoredCallSets.version: 2`; embedded `ReviewEvidence.schema_version: 1` | ops `StoredReview`, `StoredCallSets`, `ReviewEvidence`; docs specialized Sanger payload; no schema | StoredReview/ReviewEvidence extras kept; call-set envelope rejects unknowns; nested payload rules differ |
| `draft.json` and embedded `draft_json`: automatic Sanger draft, trace evidence | A / embedded review | `save_durable` retains exact draft bytes produced by plugin | `SangerReviewSession::from_draft`, `basecraft_docs::sanger::SangerDraft` | No schema version; `version_output` is tool output, not format version | docs `SangerDraft`; no protocol schema | Existing typed draft fields drop unknowns on typed reserialization; original JSON retained in review |
| Sanger placement artifact (retained JSON) | A / plugin output | Registered plugin produces it; Sanger storage retains/copies it | storage placement verification, docs `SangerPlacement` | Existing producer format; tool `version` is not a schema version | docs `SangerPlacement`; no schema | Existing typed payload behavior; no migration here |
| Sanger `request.json`, `parameters.json` | A | storage `ReviewRun` allocation / parameters writer | Sanger provenance/Methods and retained evidence readers | Request has no version; parameters are operation-specific | `Value`; request is **not** registered `OperationRequest` | Raw parameters retained; not rewritten through a common schema |
| Sanger `execution.json` and J `manifest.json`: status, owner, source, result/diagnostic, run path | A / J | storage `ReviewRun` status writer | `document_record::jobs`, Sanger resume/storage readers | `schema_version: 1` | `Value`; no schema | Selective reads; status updates build fresh record |
| Sanger recovery `manifest.json`: owner, source, unresolved diagnostic, checkpoint path | R (job recovery) | storage `checkpoint` | job/history inspection and explicit Sanger recovery | `schema_version: 1` | `Value`; no schema | Selective reads; regenerated at checkpoint |

## Registered operations, jobs and exports

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| `descriptor.json`: accepted operation descriptor | A | [runner.rs](../crates/basecraft-plugin-host/src/runner.rs) `PreparedOperation::prepare` | repair, `document_record::task_primary_output` | Descriptor semantic `version` string (not a schema-version field) | protocol `OperationDescriptor` within [operation-contract.schema.json](operation-contract.schema.json) | Descriptor rejects unknown fields; parameters schema is open JSON |
| `request.json`: accepted registered request | A | `PreparedOperation::prepare` | repair/retained run inspection; submission qualification | `schema: 1` | protocol `OperationRequest` / [operation-request.schema.json](operation-request.schema.json) | Request envelope rejects unknowns; parameter object preserved |
| `execution.json`: plugin identity/pins/runtime/context, initiator/executor, requested/effective parameters, tool/process evidence | A | `PreparedOperation::execute` | `verify_completed_result`, `record_document_run`, submission qualification | `schema: 1` | protocol `records::ExternalExecution` / [external-execution.schema.json](external-execution.schema.json) | Unchanged Value readers; typed round-trip retains nested snapshot extras; new execution builds fresh record |
| Host-workflow `execution.json`: workflow/initiator/executor/pins/parameters | A | `PreparedOperation::execute_workflow` | same execution-evidence readers | `schema: 1` | protocol `records::HostWorkflowExecution` / [host-workflow-execution.schema.json](host-workflow-execution.schema.json); distinct external shape | Selective reads unchanged; typed round-trip retains extras; fresh record |
| `runtime.json`: operation lifecycle/outcome/validated artifacts/errors/pins | A | `PreparedOperation::persist` (jobs drive transitions) | `lookup_completed_result`, `jobs` repair, `document_record::reconcile_task_receipt` | `schema: 1` | protocol `OperationOutcome` / [operation-outcome.schema.json](operation-outcome.schema.json) | Outcome/error/artifact structs drop unknowns; nested pins reject them; JSON data/parameters retained |
| `manifest.json`: lightweight job index with runtime receipt pointer | J | `PreparedOperation::write_task_index` | `document_record::job_records`, receipt reconciliation | `schema_version: 1` | protocol `records::LocalJobIndex` / [local-job-index.schema.json](local-job-index.schema.json); **not** `JobRecord` | Unchanged selected-field reads; typed round-trip retains extras; rebuilt index, validated runtime receipt authoritative |
| `validated.json`: plugin's accepted invocation result | A | `PreparedOperation::validate_result`; [jobs.rs](../crates/basecraft-plugin-host/src/jobs.rs) output validation | finalization/repair | No schema version in current `InvokeResult` | protocol plugin `InvokeResult` / [plugin-invoke-result.schema.json](plugin-invoke-result.schema.json) | Unknown typed fields dropped; plugin data JSON retained |
| `validated-artifacts.json`: validated artifact array | A | Same output-validation paths | finalization/repair | No outer version; run contract supplies context | `Vec<(String, PathBuf, String)>` role/path/digest tuples; no schema | Tuple length/types checked; no object extras |
| `stages.json`: child operation outcome array | A workflow | [delimitation_pipeline.rs](../crates/basecraft-plugin-host/src/delimitation_pipeline.rs) `AlignmentWorkflow::persist` | runner workflow repair/reconciliation | No outer version; each outcome `schema: 1` | `Vec<OperationOutcome>`; outcome schema for each item | Same as outcome |
| `query-preparation.json`: query/remote submission plan | A | `PreparedOperation::prepare` | runner lifecycle validation/receipt handling | BLAST plan's existing `schema: 1`; operation-dependent plan payload | ops `blast::Plan` or operation plan JSON; no unified schema | BLAST plan structs drop extras; raw remote plan uses Value |
| `collection-<role>-intent.json`: expected member map | A | `PreparedOperation::finalize` | same finalizer / `verify_collection` | `schema: 1` | `Value`; no schema | Selected fields; intent created once, verified on retry |
| `collection-<role>-export.json`: destination and expected members | A | `PreparedOperation::finalize` | finalizer receipt checks | `schema: 1` | `Value`; no schema | Selected fields; no general rewrite |
| `member-<hash>-export.json`: destination and content hash | A | `export_member` | `verify_member_receipt` | `schema: 1` | `Value`; no schema | Selected fields; no general rewrite |
| Cancellation flag file | A | `jobs::cancel_job` atomic write | plugin cancellation context / process monitor | No version; marker bytes | Not JSON | Not applicable |
| Workflow `fit.json`, unavailable `fit.json.run.json` | A staged/published artifacts | `AlignmentWorkflow::run` copies/augments registered stage output | `files::load_delimitation`, workflow/runner output validation | Report `version: 1` where present; producer-dependent fit payload | Protocol `DelimitationResult`/`JobRecord` for compatible reports; augmented `Value`; [delimitation-result.schema.json](delimitation-result.schema.json), [job-record.schema.json](job-record.schema.json) cover typed forms only | Preserve Value while augmenting; typed reader rules vary |
| Plugin scientific outputs: reports, input evidence, collections, tool stdout/stderr and native files | A `pending` / `artifacts`, declared destinations | Registered plugin; runner freezes, validates and publishes exact bytes | Contract validation, format-specific readers, artifact digest verification | Producer/standard-specific; declared in operation output contract | Public `PluginArtifact`, `PublishedArtifact`, `ArtifactRevision`; arbitrary scientific JSON is **not** a host record schema | Opaque artifacts remain byte-preserved; do not impose host record fields |
| `package.json`: GenBank submission export manifest | E submission package | `submission::export`, `record_validation` | `read_validation`, package validation/receipt qualification | `schema` format identifier, `version: 1` | `Value`; no schema | Existing Value retained while updating validation fields; fresh export rebuilds |
| `validation/<input-hash>/summary.json`: qualified table2asn outcome/issues/copies | E submission package | `submission::record_validation` | `read_validation`, package status/readme generation | No version field | ops `ValidationSummary`; no schema | Unknown struct fields dropped |
| Submission tables, issues, README and standard `.fsa`, `.tbl`, `.src`, `.sbt`, `.sqn`, `.gbf`, raw tool reports | E submission package | `export`, `record_validation`, standard IO writers / tool | package validation, external submitter, human | Standard formats / package manifest | Standard text/binary / typed issue rows, not new JSON formats | Exact file hashes recorded; original tool report bytes retained |

## Related setup/cache records (not portable project provenance)

| Record | Location | Writer / reader | Version; type/schema | Unknown fields |
|---|---|---|---|---|
| `plugins-v1.json` | C config | [registry.rs](../crates/basecraft-plugin-host/src/registry.rs) `write_index` / `load_index` | `schema: 1`; host `InstallationIndex`; no schema | `unknown` map kept; nested runtime rejects unknowns |
| Managed runtime `runtime.json` | C plugin runtime directory | [runtime.rs](../crates/basecraft-plugin-host/src/runtime.rs) runtime builder / `Runtime::verify` and registry loading | `schema: 1`; host `Runtime`; no schema; distinct from run outcome | Rejected |
| Managed `pixi.toml`, `pixi.lock`, `.pixi/config.toml` | C runtime staging / installed runtime | runtime builder from manifest/locked source; Pixi and runtime verifier read | Existing Pixi formats; `RuntimeSpec` setup; no host JSON schema | Manifest generated; pinned lock copied and verified, not reinterpreted |
| `ncbi-contact.json` | C state | [lifecycle.rs](../crates/basecraft-plugin-host/src/lifecycle.rs) `reserve`, `ContactPermit::complete` read/write | Bare numeric timestamp; no version or wrapper | Not an object; no unknown fields |
| `preferences.toml` | C config | platform [settings.rs](../crates/basecraft-platform/src/settings.rs) `SettingsStore` load/update | Typed versioned settings; [preferences-format.md](preferences-format.md) | Extra TOML fields preserved by store; application preferences, not project record |
| Content-hash cache entries, FASTA index cache entries | C cache | IO [cache.rs](../crates/basecraft-io/src/cache.rs) `content_hash`, `open_fasta_in` / matching readers | Hash text / binary FASTA index; no JSON envelope | Disposable cache formats; do not turn into project provenance |
| Save/evidence/contact/settings lockfiles | C state / temporary save-lock folder | `save_guard::lock`, `input_evidence::lock`, lifecycle contact lock, `SettingsStore` lock | OS lock handle files; no serialized record version | Not data records |

`project.toml`, plugin manifests/operation catalogs, installation `build.json` and
local installation receipts are read by these crates but are not written by
ops/plugin-host. They remain governed by their existing project/plugin/build
contracts. Standard primary document writers (FASTA, GenBank, GFF3, alignments,
Newick/NEXUS, SPART) and opaque retained `objects/<hash>` / `artifacts/<hash>` bytes
are covered by their format readers and artifact hashes, not by a new JSON schema.
Arrow snapshots (`snapshot.arrow-pair`) retain the existing protocol buffer
contract; shared-memory transport buffers are not durable record stores.

## First typed slice and remaining work

`RecoveryManifest` and `PendingSave` replace only their existing session builders.
Both remain schema version 1 with exactly the same keys, nulls, array order,
recursive key ordering and no trailing newline. Round-trip and unknown-field tests
use captured [records-v1 examples](../testdata/fixtures/records-v1/README.txt).
Actual writer tests exercise first edit, existing-target partial save, retry,
replay and new-target partial save, comparing all bytes with the original literal
builders. Runtime paths, IDs and timestamps are not normalized in those checks.

Reuse assessment: `LogEntry`, `Publication`, `InputReference`, `FileStamp`,
`OperationRequest`, `OperationOutcome`, `InvokeResult`, `PublishedArtifact`,
`JobRecord` and the metadata/submission/workspace models already exist in protocol.
Do not create alternate versions of these. Recovery baseline JSON deliberately
retains the complete original log snapshot: today's `LogEntry` does not retain
unknown event/operation fields. Its schema reuses the shared LogEntry description.
New type readers retain unknown status strings and top-level fields. Existing
replay and GUI readers remain unchanged and validate versions as before.

Every row marked `Value`, literal bytes, local ops/host type, no schema, or payload
schema only is remaining M6 work, not complete coverage by this slice. Several
legacy files lack `schema_version`, use `version`, `schema`, or `v`, or are arrays
or scalars. Those are existing architecture-brief section 13 debt. Adding version
fields now would change bytes; later changes need explicitly approved versioning
and replay/compatibility treatment. Existing unknown-field loss/rejection and
string-valued evidence/job states are also listed debt, not silently fixed here.

Future additive work is separate: record genetic-code inputs (F2), branch-length
meaning/source (F5), relative invocation paths/executor contracts (F8), and plugin
environment specification plus equivalent operations request (F11). None authorizes
adding fields to this byte-compatible slice. Module moves, registry changes,
runner stages, error routing, writer-version replay policy and the user's format
compatibility decision remain later coordinated tasks.

## Typed analysis-store slice

`StoreMarker` and `StoreRegistry` describe the existing version-one namespace and
local known-store registry. `StoreFormat` names the existing namespace identifier.
The marker still uses compact sorted JSON plus one trailing newline; registry
writes still use recursively sorted pretty JSON plus one trailing newline.
`store_registry_formats.rs` ran against the original writers before replacement
and compares complete actual writer bytes without normalizing runtime paths.
Committed examples retain those bytes except for synthetic fixture roots; they
include nested unknown fields, an empty registry and successive registrations.
The empty registry is an in-memory default, not a newly created file: there was
no original production writer for an empty registry.

Marker safety/version checks (4096 bytes), registry read bounds (1 MiB), absolute
paths, 4096-store count, lock coverage and atomic write/sync behavior are unchanged.
Registry paths remain absolute LOCAL machine state. Unknown top-level fields
survive registration updates, including nested JSON values. Marker extras survive
typed round-trip, but validation does not rewrite the marker. Existing selected
field/version checks still precede typed parsing to retain error behavior.

This is a shared type for later registry work, not registry version two: reverse
indexes, per-store locks, save warning/default policy and migration remain M3.
No new fields, formats, entry routes or compatibility policy are introduced.

## Typed execution and local job-index slice

`ExternalExecution` and `HostWorkflowExecution` name the two existing
`execution.json` shapes explicitly, with no permissive union. `LocalJobIndex`
names the small application-state index, not the scientific `JobRecord` or the
authoritative `OperationOutcome` in `runtime.json`. RuntimeState supplies the
index's existing state strings. Writers serialize through Value to retain
recursive key sorting and pretty JSON with exactly one final newline. Existing
`schema: 1` versus `schema_version: 1`, explicit nulls, omitted workflow-only
nonfields, path conversions, fresh-record behavior and all readers are unchanged.
Unknown fields survive typed round-trip; no production merge/rewrite is implied.

Reuse assessment: requests/outcomes remain OperationRequest/OperationOutcome;
initiator is the existing string label, not Actor's tagged shape. Context, tool
and execution snapshots reference public InvocationContext/ToolInfo/ExecutionInfo
in generated schemas. Their Rust fields retain opaque JSON because those public
types reject or discard nested extensions (including input evidence and methods).
Production constructs those snapshots from the same public values. Managed-runtime
receipts have no public protocol payload: they stay opaque to avoid an Apache
protocol dependency on the GPL host implementation. Parameters remain arbitrary
operation-schema JSON. The small plugin identity and source wrapper have no
shape-compatible public type: Actor, OperationPins and hashed InputReference
would add or rename fields. Index outputs reuse the existing path-to-digest map.

Generated schemas describe current known payloads, not future-extension validation
or the existing readers' selected-field acceptance policy. Version, ownership,
receipt/hash verification and failure diagnostics are still enforced at the same
production boundaries. No new format, compatibility promise, lifecycle transition,
actor, dependency, cancellation, override or immutable-artifact behavior is added.
Parent M6 remains incomplete.
