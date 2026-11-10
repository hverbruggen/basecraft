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
them when writing; “rejected” means `deny_unknown_fields`. This documentation audit changes none of those behaviors. Earlier typed slices
are described below; their readers and fresh-write behavior remain distinct.

## Recovery, journal and provenance

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| **RP01** `recovery.json`: session, source root, baseline event, retention limit, status | R (legacy sessions: project root) | [session.rs](../crates/basecraft-ops/src/session.rs) `Session::track` | [replay.rs](../crates/basecraft-ops/src/replay.rs) `recorded_source_root`; GUI `provenance_bridge::recovery_sessions` | `schema_version: 1` | protocol `records::RecoveryManifest`; [recovery-manifest.schema.json](recovery-manifest.schema.json) | Kept at top level and throughout opaque baseline; readers inspect selected fields |
| **RP02** `pending-save.json`: session, target, before hash or null, ordered expected hashes, status, note | R | `Session::save_checked` | save success removes it; `Session` drop/cleanup preserves recovery if present; user/repair tools inspect it | `schema_version: 1` | protocol `records::PendingSave`; [pending-save.schema.json](pending-save.schema.json) | Kept by type; writer creates a fresh intent, does not merge an earlier intent |
| **RP03** `.basecraft/log/<stream>/<month>-<id>-<segment>.jsonl`: checksummed, sequenced events | R / project | [log.rs](../crates/basecraft-ops/src/log.rs) `LogWriter::append_batch` | `read_all`, `read_readable`, `verified_prefix`, `parse_entry`; replay | `v: 1`; `_checksum`, `_sequence` envelope | protocol `LogEntry` / [log-entry.schema.json](log-entry.schema.json); checksum/sequence envelope untyped | Envelope verified then stripped; unknown event fields dropped by `LogEntry`; future operation variants rejected |
| **RP04** `recovery-integrity-error.json`: verified length and durability diagnostic | R / project | `LogWriter::append_batch` failed rollback | `replay::recover` refuses poisoned recovery | `schema_version: 1` | protocol `records::RecoveryIntegrityError` / [recovery-integrity-error.schema.json](recovery-integrity-error.schema.json) | Typed round-trip keeps extras; reader checks presence; no rewrite |
| **RP05** `manifest.json`, `records/<publication-id>.json`: publication, ancestry, artifacts, events, effective edits, completeness | P / A | [provenance.rs](../crates/basecraft-ops/src/provenance.rs) `publish`, `publish_operation`; session publication routines | `inspect`, `verify`, `package`; `document_record::read_origin`, evidence ancestry readers | `schema_version: 1` | protocol `Publication` / [provenance-publication.schema.json](provenance-publication.schema.json) | Rejected at publication and several nested record types; `InputReference.extra` kept |
| **RP06** `input.json`: source evidence, submitted hash/size, transformation, evidence state, optional ancestry/ledger references | A / P | [input_evidence.rs](../crates/basecraft-ops/src/input_evidence.rs) `reference_input_with_dependencies`, `link_generated_input`, `resolve`, `relink`; session/provenance/runner/Sanger adapters | `read_input`, `conflicts_in`, `verified_source`, packaging/ancestry readers | `schema_version: 1` | ops `SubmittedInput`; reuses protocol `InputReference`, `FileStamp`; no own schema | Kept via `extra` on input/reference; source stamp has no extra map; evidence state still string |
| **RP07** `run.json`: id, result name, status and sometimes outputs | A | `Session::save_inner`, `publish_referenced_analysis`, `publish_extraction`; `input_evidence` reference/generated helpers; runner `prepare` | `input_evidence::result_name`, `scan_run`; artifact binding | `schema_version: 1` | protocol `records::AnalysisRun` / [analysis-run.schema.json](analysis-run.schema.json) | Selective `Value` reads unchanged; typed round-trip retains unknown status/extras; outputs omitted or array, result null or string; fresh records |
| **RP08** `store.json`: analysis namespace marker | `.basecraft/analyses-v1` | [analysis_store.rs](../crates/basecraft-ops/src/analysis_store.rs) `ArtifactRun::create`, compact JSON plus newline | `ArtifactRun::validate_store` | `format: basecraft.analysis-store`, `schema_version: 1` | protocol `StoreMarker` / [store-marker.schema.json](store-marker.schema.json) | Top-level extras kept by type; validation ignores extras; marker not rewritten |
| **RP09** Result binding (hash-derived filename): run, result filename/hash | `.basecraft/analyses-v1` binding namespace | `ArtifactRun::bind` | `ArtifactRun::for_result` | `schema_version: 1` | protocol `records::ResultBinding` / [result-binding.schema.json](result-binding.schema.json) | Typed round-trip keeps extras; selective reads; fresh writes |
| **RP10** `analysis-stores.json`: absolute known store paths | C state | `input_evidence::register` | `store_registry`, `stores`, save conflict scan | `schema_version: 1` | protocol `StoreRegistry` / [store-registry.schema.json](store-registry.schema.json) | Top-level extras kept on registration update; absolute path array/count validated |
| **RP11** `artifacts/<hash>` with logical name `export-map.json`: old/new publication mapping | E packaged P | `input_evidence::export_history` | retained artifact inspection; no dedicated in-app deserializer | `schema_version: 1` | `Value`; no schema | Generated artifact; original records also retained, no rewrite reader |
| **RP12** `reports/manifest.json`: publication, template, output hashes, redactions, citations | P / A | `provenance::write_reports` | report consumer / inspection (no dedicated typed in-app reader) | `schema_version: 1`, `template_version: 2` | `Value`; no schema | Generated afresh |
| **RP13** `reports/source-map.json`: paragraph/activity array | P / A | `provenance::write_reports` | report consumer / inspection | No version field | `Vec<Value>`; no schema | Generated afresh |
| **RP14** `reports/history.md`, `methods.generated.md`, `parameters.tsv`, bundle `README.md` | P / A | `provenance::write_reports`, `publish` | human/report consumers | Report manifest template version | Markdown / TSV, not JSON records | Not applicable; user-authored `reports/methods.md` is preserved, never regenerated |
| **RP15** `record.json`: document chronology and effective operations | E report folder | [document_record.rs](../crates/basecraft-ops/src/document_record.rs) `Report::write_files` (`raw`) | `DocumentRecord` consumers / exported evidence inspection | `schema_version: 1` | ops `DocumentRecord`; no generated schema | Dropped by struct; events retain existing LogEntry behavior |
| **RP16** `report.json`: history, paragraphs, methods, raw record, citations | E report folder | `Report::write_files` | report consumer | `schema_version: 1`, `template_version` | ops `Report`; no schema | Dropped by struct |
| **RP17** `manifest.json`: report revision/document/unsaved/template/coordinates | E report folder | `Report::write_files` | `check_report_folder` before replacement | `schema_version: 1` | `Value`; no schema | Selected ownership fields read; regenerated |
| **RP18** `source-map.json`, `citations.json`: paragraph map array / citation string array | E report folder | `Report::write_files` | report consumers | No version fields | `Vec<Value>` / `Vec<String>`; no schemas | Generated afresh |
| **RP19** `history.md`, `methods.generated.md`, `parameters.tsv` | E report folder | `Report::write_files` | report consumers | Companion manifest template version | Markdown / TSV | Generated afresh; ownership check protects unrelated folders/files |

## Document companions and Sanger payloads

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| **DS01** `<tree>.basecraft.json`: exchange/tree hashes, supports, annotations, style | Beside tree / A hidden metadata | [files.rs](../crates/basecraft-ops/src/files.rs) `write_tree_sidecar`, `write_tree_sidecar_at`; plugin-host `file_tool::write_support_sidecar` | `files::load_tree` | `schema_version: 1`; missing/0 legacy accepted with restricted semantics; historical writer omits version | ops `TreeSidecar`; protocol support/style payloads; no complete schema | Outer unknown fields dropped; nested style `extra` kept; legacy backup retained before replacement |
| **DS02** `<species>.basecraft.json`: exchange hash, path base, workspace | Beside SPART / A | `files::write_delimitation_sidecar`, `write_delimitation_sidecar_at`; [delimitation.rs](../crates/basecraft-ops/src/delimitation.rs) artifact writer; session save | `files::load_delimitation` | `version: 1` (legacy relative form) / `2` (path base) | ops `DelimitationSidecar`; protocol `DelimitationWorkspace` / [delimitation-workspace.schema.json](delimitation-workspace.schema.json) covers payload only | Outer fields dropped; payload's own serde rules apply |
| **DS03** `<tree>.alignment.json`, hidden origin/view link records | Beside tree / A | [links.rs](../crates/basecraft-ops/src/links.rs) `AlignmentLink::create`, `create_referenced`, association/clone/save helpers | `AlignmentLink` loading/verification; `PreparedLinks::prepare` | `schema: 1` snapshot / `2` referenced ledger | protocol `records::AlignmentLink` (ops re-export, behavior via `AlignmentLinkExt`); [alignment-link.schema.json](alignment-link.schema.json) | Top-level `extra` kept |
| **DS04** Retained `*.alignment-recovery.json`: prior/intended origin/view bytes | Beside save target, uniquely retained | `PreparedLinks::publish` | manual repair / diagnostic inspection; no automatic recovery writer | `schema: 1` | protocol `link_records::AlignmentLinkRecovery`; [alignment-link-recovery.schema.json](alignment-link-recovery.schema.json) | Generated snapshot, not rewritten; no extension-preserving repair interface |
| **DS05** Origin/view unavailable guard: diagnostic and recovery filename | Same alignment sidecar locations during partial save | `PreparedLinks::publish` | link loading refuses unusable association | `schema: 1` | protocol `link_records::AlignmentLinkUnavailable`; [alignment-link-unavailable.schema.json](alignment-link-unavailable.schema.json) | Generated guard, not ordinary AlignmentLink; selected-field Value reader unchanged |
| **DS06** `metadata.basecraft.json`: table names/hashes/columns, import settings, partitions | Project metadata folder | [metadata.rs](../crates/basecraft-ops/src/metadata.rs) `encode`, `write` | `metadata::load` | `format`, `version: 1` | ops `Manifest`, `Tables`, `TableEntry`; protocol `ProjectMetadata` / [project-metadata.schema.json](project-metadata.schema.json) covers model, not disk manifest | Manifest top-level extra kept; nested table wrappers drop extras; unknown TSV columns retained by model |
| **DS07** `specimens.tsv`, `taxa.tsv`, `identifications.tsv`, `records.tsv` | Same metadata folder | `metadata::encode`, `create`, session save | `metadata::load` | Via metadata manifest | protocol metadata row/column types; standard TSV | Header/column handling preserves extra data; no JSON schema for TSV bytes |
| **DS08** `*.submission.json` | Project / chosen document path | [submission.rs](../crates/basecraft-ops/src/submission.rs) `encode`, `create`; session save | `submission::load` | `version: 1` | protocol `Submission` / [submission.schema.json](submission.schema.json) | Extra maps on Submission and selected nested types kept; other nested structs have their own rules |
| **DS09** Saved `*.sanger-review.json` / recovery `review.sanger-review.json` / immutable retained review snapshots | Document / R / A | [sanger_review/storage.rs](../crates/basecraft-ops/src/sanger_review/storage.rs) `write_embedded`, `save_durable`, checkpoint | [sanger_review.rs](../crates/basecraft-ops/src/sanger_review.rs) `decode`, `from_stored`, `load`, `recover` | `StoredReview.version: 1`; `StoredCallSets.version: 2`; embedded `ReviewEvidence.schema_version: 1` | ops `StoredReview`, `StoredCallSets`, `ReviewEvidence`; docs specialized Sanger payload; no schema | StoredReview/ReviewEvidence extras kept; call-set envelope rejects unknowns; nested payload rules differ |
| **DS10** `draft.json` and embedded `draft_json`: automatic Sanger draft, trace evidence | A / embedded review | `save_durable` retains exact draft bytes produced by plugin | `SangerReviewSession::from_draft`, `basecraft_docs::sanger::SangerDraft` | No schema version; `version_output` is tool output, not format version | docs `SangerDraft`; no protocol schema | Existing typed draft fields drop unknowns on typed reserialization; original JSON retained in review |
| **DS11** Sanger placement artifact (retained JSON) | A / plugin output | Registered plugin produces it; Sanger storage retains/copies it | storage placement verification, docs `SangerPlacement` | Existing producer format; tool `version` is not a schema version | docs `SangerPlacement`; no schema | Existing typed payload behavior; no migration here |
| **DS12** Sanger `request.json`, `parameters.json` | A | storage `SangerRun::prepare_with_dependencies`, `parameters` | Sanger provenance/Methods and retained evidence readers | Request has no version; parameters are operation-specific | `Value`; request is **not** registered `OperationRequest` | Raw parameters retained; not rewritten through a common schema |
| **DS13** Sanger `execution.json` and J `manifest.json`: status, owner, source, result/diagnostic, run path | A / J | storage `SangerRun::status` | `document_record::jobs`, Sanger resume/storage readers | `schema_version: 1` | `Value`; no schema | Selective reads; status updates build fresh record |
| **DS14** Sanger recovery `manifest.json`: owner, source, unresolved diagnostic, checkpoint path | R (job recovery) | storage `checkpoint` | job/history inspection and explicit Sanger recovery | `schema_version: 1` | `Value`; no schema | Selective reads; regenerated at checkpoint |

## Registered operations, jobs and exports

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| **EX01** `descriptor.json`: accepted operation descriptor | A | [runner.rs](../crates/basecraft-plugin-host/src/runner.rs) `PreparedOperation::prepare` | repair, `document_record::task_primary_output` | Descriptor semantic `version` string (not a schema-version field) | protocol `OperationDescriptor` within [operation-contract.schema.json](operation-contract.schema.json) | Descriptor rejects unknown fields; parameters schema is open JSON |
| **EX02** `request.json`: accepted registered request | A | `PreparedOperation::prepare` | repair/retained run inspection; submission qualification | `schema: 1` | protocol `OperationRequest` / [operation-request.schema.json](operation-request.schema.json) | Request envelope rejects unknowns; parameter object preserved |
| **EX03** `execution.json`: plugin identity/pins/runtime/context, initiator/executor, requested/effective parameters, tool/process evidence | A | `PreparedOperation::execute` | `verify_completed_result`, `record_document_run`, submission qualification | `schema: 1` | protocol `records::ExternalExecution` / [external-execution.schema.json](external-execution.schema.json) | Unchanged Value readers; typed round-trip retains nested snapshot extras; new execution builds fresh record |
| **EX04** Host-workflow `execution.json`: workflow/initiator/executor/pins/parameters | A | `PreparedOperation::execute_workflow` | same execution-evidence readers | `schema: 1` | protocol `records::HostWorkflowExecution` / [host-workflow-execution.schema.json](host-workflow-execution.schema.json); distinct external shape | Selective reads unchanged; typed round-trip retains extras; fresh record |
| **EX05** `runtime.json`: operation lifecycle/outcome/validated artifacts/errors/pins | A | `PreparedOperation::persist` (jobs drive transitions) | `lookup_completed_result`, `jobs` repair, `document_record::reconcile_task_receipt` | `schema: 1` | protocol `OperationOutcome` / [operation-outcome.schema.json](operation-outcome.schema.json) | Outcome/error/artifact structs drop unknowns; nested pins reject them; JSON data/parameters retained |
| **EX06** `manifest.json`: lightweight job index with runtime receipt pointer | J | `PreparedOperation::write_task_index` | `document_record::job_records`, receipt reconciliation | `schema_version: 1` | protocol `records::LocalJobIndex` / [local-job-index.schema.json](local-job-index.schema.json); **not** `JobRecord` | Unchanged selected-field reads; typed round-trip retains extras; rebuilt index, validated runtime receipt authoritative |
| **EX07** `validated.json`: plugin's accepted invocation result | A | `PreparedOperation::validate_result`; [jobs.rs](../crates/basecraft-plugin-host/src/jobs.rs) output validation | finalization/repair | No schema version in current `InvokeResult` | protocol plugin `InvokeResult` / [plugin-invoke-result.schema.json](plugin-invoke-result.schema.json) | Unknown typed fields dropped; plugin data JSON retained |
| **EX08** `validated-artifacts.json`: validated artifact array | A | Same output-validation paths | finalization/repair | No outer version; run contract supplies context | `Vec<(String, PathBuf, String)>` role/path/digest tuples; no schema | Tuple length/types checked; no object extras |
| **EX09** `stages.json`: child operation outcome array | A workflow | [delimitation_pipeline.rs](../crates/basecraft-plugin-host/src/delimitation_pipeline.rs) `AlignmentWorkflow::persist` | runner workflow repair/reconciliation | No outer version; each outcome `schema: 1` | `Vec<OperationOutcome>`; outcome schema for each item | Same as outcome |
| **EX10** `query-preparation.json`: query/remote submission plan | A | `PreparedOperation::prepare` | runner lifecycle validation/receipt handling | BLAST plan's existing `schema: 1`; operation-dependent plan payload | ops `blast::Plan` or operation plan JSON; no unified schema | BLAST plan structs drop extras; raw remote plan uses Value |
| **EX11** `collection-<role>-intent.json`: expected member map | A | `PreparedOperation::finalize` | finalizer checks intent presence; `verify_collection` checks retained collection against reconstructed expected map | `schema: 1` | protocol `publication_records::CollectionIntent`; [collection-intent.schema.json](collection-intent.schema.json) | Intent contents are not parsed on retry; created before publication, no general rewrite |
| **EX12** `collection-<role>-export.json`: destination and expected members | A | `PreparedOperation::finalize` | finalizer checks export intent presence; `verify_collection` checks destination against reconstructed expected map | `schema: 1` | protocol `publication_records::CollectionExportIntent`; [collection-export-intent.schema.json](collection-export-intent.schema.json) | Intent contents are not parsed on retry; no general rewrite |
| **EX13** `member-<hash>-export.json`: destination and content hash | A | `export_member` | `verify_member_receipt` parses selected schema/destination/hash/device/inode/size fields and verifies destination bytes | `schema: 1` | protocol `publication_records::MemberExportReceipt`; [member-export-receipt.schema.json](member-export-receipt.schema.json) | Unknown JSON fields ignored by selected-field checks; no general rewrite |
| **EX14** Cancellation flag file | A | `jobs::cancel_job` atomic write | plugin cancellation context / process monitor | No version; marker bytes | Not JSON | Not applicable |
| **EX15** Workflow `fit.json`, unavailable `fit.json.run.json` | A staged/published artifacts | `AlignmentWorkflow::run` copies/augments registered stage output | `files::load_delimitation`, workflow/runner output validation | Report `version: 1` where present; producer-dependent fit payload | Protocol `DelimitationResult`/`JobRecord` for compatible reports; augmented `Value`; [delimitation-result.schema.json](delimitation-result.schema.json), [job-record.schema.json](job-record.schema.json) cover typed forms only | Preserve Value while augmenting; typed reader rules vary |
| **EX16** Plugin scientific outputs: reports, input evidence, collections, tool stdout/stderr and native files | A `pending` / `artifacts`, declared destinations | Registered plugin; runner freezes, validates and publishes exact bytes | Contract validation, format-specific readers, artifact digest verification | Producer/standard-specific; declared in operation output contract | Public `PluginArtifact`, `PublishedArtifact`, `ArtifactRevision`; arbitrary scientific JSON is **not** a host record schema | Opaque artifacts remain byte-preserved; do not impose host record fields |
| **EX17** `package.json`: GenBank submission export manifest | E submission package | `submission::export`, `record_validation` | `read_validation`, package validation/receipt qualification | `schema` format identifier, `version: 1` | `Value`; no schema | Existing Value retained while updating validation fields; fresh export rebuilds |
| **EX18** `validation/<input-hash>/summary.json`: qualified table2asn outcome/issues/copies | E submission package | `submission::record_validation` | `read_validation`, package status/readme generation | No version field | ops `ValidationSummary`; no schema | Unknown struct fields dropped |
| **EX19** Submission tables, issues, README and standard `.fsa`, `.tbl`, `.src`, `.sbt`, `.sqn`, `.gbf`, raw tool reports | E submission package | `export`, `record_validation`, standard IO writers / tool | package validation, external submitter, human | Standard formats / package manifest | Standard text/binary / typed issue rows, not new JSON formats | Exact file hashes recorded; original tool report bytes retained |

## Related setup/cache records (not portable project provenance)

| Record | Location | Writer / reader | Version; type/schema | Unknown fields |
|---|---|---|---|---|
| **LC01** `plugins-v1.json` | C config | [registry.rs](../crates/basecraft-plugin-host/src/registry.rs) `write_index` / `load_index` | `schema: 1`; host `InstallationIndex`; no schema | `unknown` map kept; nested runtime rejects unknowns |
| **LC02** Managed runtime `runtime.json` | C plugin runtime directory | [runtime.rs](../crates/basecraft-plugin-host/src/runtime.rs) runtime builder / `Runtime::verify` and registry loading | `schema: 1`; host `Runtime`; no schema; distinct from run outcome | Rejected |
| **LC03** Managed `pixi.toml`, `pixi.lock`, `.pixi/config.toml` | C runtime staging / installed runtime | runtime builder from manifest/locked source; Pixi and runtime verifier read | Existing Pixi formats; `RuntimeSpec` setup; no host JSON schema | Manifest generated; pinned lock copied and verified, not reinterpreted |
| **LC04** `ncbi-contact.json` | C state | [lifecycle.rs](../crates/basecraft-plugin-host/src/lifecycle.rs) `reserve`, `ContactPermit::complete` read/write | Bare numeric timestamp; no version or wrapper | Not an object; no unknown fields |
| **LC05** `preferences.toml` | C config | platform [settings.rs](../crates/basecraft-platform/src/settings.rs) `SettingsStore::{load,update,import_qsettings}` | platform `Preferences`, `schema_version: 1`; [preferences-format.md](preferences-format.md) | Extra TOML fields preserved by store; application preferences, not project record |
| **LC06** Content-hash cache entries, FASTA index cache entries | C cache | IO [cache.rs](../crates/basecraft-io/src/cache.rs) `content_hash_in`, `open_fasta_in` / matching readers | Hash text / binary FASTA index; no JSON envelope | Disposable cache formats; do not turn into project provenance |
| **LC07** Save/evidence/contact/settings lockfiles | C state / temporary save-lock folder | `save_guard::lock`, `input_evidence::lock`, lifecycle contact lock, `SettingsStore` lock | OS lock handle files; no serialized record version | Not data records |

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

## Typed recovery-integrity and result-binding slice

`RecoveryIntegrityError` and `ResultBinding` describe two existing version-one
objects, without changing their writers' bytes or readers' acceptance rules.
The poison marker retains pretty recursively sorted JSON with no final newline;
the binding retains pretty recursively sorted JSON with one final newline.
Both serialize through Value before the existing atomic writers. Small known
fields are typed directly, with flattened extras retained on typed round-trip.
There are no null or optional fields in either original shape, and none are added.

The log marker is a diagnostic after failed rollback durability, not another log.
Append sequencing, rollback, error handling and the best-effort marker write stay
unchanged. Both recovery routes still refuse the marker's presence without
parsing its contents, including empty, damaged or future-version contents.
`ResultBinding` is not an `InputReference` (it has a run and relative result
filename) or a `Publication`. Its result stays the existing UTF-8 string converted
from a relative path. Fresh binding writes do not merge previous extras.

Original-writer captures cover empty/nonempty verified journal prefixes and
ordinary/nested Unicode result filenames. Full-byte runtime oracles retain the
original literals and compare current writer bytes without normalization.
Committed examples use those exact captures; the test setup uses synthetic labels
and the records contain no private absolute paths, hosts or unpredictable IDs.
The nonempty poison length is the captured test journal's byte length, not a
promise that arbitrary future log entries have that size.

Binding discovery, checked content verification, version/path/run validation,
namespace safety and missing/corrupt record behavior remain in existing selective
readers. The types and generated schemas describe current writer shapes, not
stricter read gates or a new compatibility policy. No replay, save budget, hash
API, operation, entry route, store or registry is introduced. Remaining M6 records,
module boundaries, typed errors and writer-version policy remain separate work.

## Typed analysis-run slice

`AnalysisRun` covers the existing version-one analysis-store `run.json` family;
`AnalysisRunStatus` names referenced/prepared/saved/completed and retains unknown
strings. These are distinct from `runtime.json` lifecycle states. Result is always
emitted as string or null. Outputs are omitted before referenced publication,
then present as an array (including an empty array). No field/version/policy is
added; all six constructors retain recursively sorted pretty JSON plus one newline
through Value and the existing atomic writer. Existing selective readers are
unchanged; fresh writes do not merge older extras.

Outputs reuse the public `ArtifactRevision` schema and are created from those
public values. The record retains their JSON snapshots because ArtifactRevision
rejects unknown fields: using it as a production parser would lose the required
extension compatibility. This follows the execution snapshot pattern rather than
adding a parallel artifact model. Schemas describe known writer shapes, not a
new reader acceptance gate.

Eight raw actual-writer examples were captured before substitution, without
normalization: reference (Unicode and non-UTF-8/null result), generated input,
Delimitation Save As, referenced publication (one output and empty outputs), and
runner preparation (export filename and no-export operation label). Runtime
oracles independently reconstruct every field from caller inputs and allocated
identities, never from the produced run.json. Original ULIDs remain in committed
examples; current-writer parity is checked against the original literals in the
same runtime, not by pretending cross-run random identities are constant.
Typed round-trips retain the exact captured bytes and unknown nested extras.

Inventory also found `Session::publish_extraction` writing the same run family.
A ninth raw example (`run-extracted.json`) captures this unchanged writer through
`Session::extract`, and round-trips through the same AnalysisRun type. Its capture
was taken after the other five substitutions, while extraction still matched
the baseline production source.
The user explicitly expanded scope to include this sixth constructor after its
original capture. All six constructors now use the one shared record type. Scientific producer `<result>.run.json`
files are separate formats listed above, not analysis-store run records.
SubmittedInput/evidence states, remaining records/module boundaries, typed errors,
writer-version/replay policy, runner stages and user compatibility decisions
remain M6 work. This slice does not complete M6.

## Audit boundary and record roles

The source audit below describes the source tree at `277eb84908a5` (the
inventory-only change does not alter those writers). IDs added to the existing
rows above are stable references; their earlier notes and typed-slice evidence
remain applicable. This is an inventory of production formats, not a claim that
every format has a protocol type, a golden file, or extension-safe rewriting.
The supplemental rows split formats previously hidden under EX16 and distinguish
local state from portable documents. Filenames for plugin outputs are allocated
by the operation contract; example basenames are not fixed installation paths.

Recommended roles, **not a storage migration or retention decision**:

| IDs | Candidate role | Qualification |
|---|---|---|
| RP01–RP04 | Private recovery / journal working state | Project journals also exist; do not discard them or assume a retention horizon |
| RP05–RP09, RP11–RP19 | Portable document/analysis evidence | RP08/RP09 are store infrastructure; relative references and retained objects matter; RP15–RP19 are derived exports |
| RP10 | Private working-state registry | Absolute local paths; not portable scientific provenance |
| DS01–DS03, DS06, DS09–DS11 | Public document companions / retained scientific evidence | Public means project/export content, not automatic disclosure; snapshots and embedded draft bytes remain significant |
| DS04–DS05 | Private repair candidates currently beside documents | Must accompany partial-save diagnosis; no permission to delete or move them |
| DS07–DS08 | Public primary documents | TSV bytes and submission JSON remain separate formats |
| DS12–DS13 | Retained analysis evidence plus private job index | Same Sanger status object is written in both places; the index location is private, the run copy is retained evidence |
| DS14 | Private recovery working state | Review checkpoint is recoverable user work, not disposable cache |
| EX01–EX05, EX07–EX10, EX15–EX16 | Retained analysis invocation/result evidence | Descriptor/request/validation receipts are records once persisted; outcome is authoritative for lifecycle |
| EX06, EX14 | Private task index / control state | Index is not the outcome; cancel presence is control, not provenance |
| EX11–EX13 | Private publication/retry working-state candidates | Keep until publication/retry safety policy is decided; they currently live in retained runs |
| EX17–EX19 | Public submission package | Validation and hashes qualify particular bytes, not later edits |
| LC01–LC07 | Private setup/preferences/cache/locks | Cache recreation is distinct from recovery; lock handles are not serialized user records |

### Additional state and concrete producer formats

All JSON rows without an explicit schema link below lack a generated protocol
schema for the **complete disk shape**. “No golden” means no committed
original-writer byte fixture was found, not that there are no behavioral tests.

| ID / file and location | Exact writer → reader | Type/version; extensions and rewrite | Role recommendation; fixture evidence/gap |
|---|---|---|---|
| **AU01** `recent-projects.json`, C state | GUI [settings_bridge.rs](../crates/basecraft-gui/src/settings_bridge.rs) `remember_project_path_at` → `load_project_history_at` | Local `ProjectHistory`, `version: 1`; serde drops extras; remember reloads list then rebuilds envelope; unsupported/corrupt file refused | Private paths/history; `project_history_tests` use temporary files, no golden |
| **AU02** legacy Qt settings, platform-specific QSettings backing store | QML's former settings writer; current [Main.qml](../crates/basecraft-gui/qml/Main.qml) `legacyPreferences` reads `ui` category → `settings_bridge::import_legacy_values`, `SettingsStore::import_qsettings` | Legacy Qt INI/native store, no Basecraft record version; import writes LC05 and leaves legacy values independent | Private preferences; real Qt migration checks described in [preferences-format.md](preferences-format.md), no cross-platform byte golden |
| **AU03** `activity/activity-<started>.txt`, C cache | GUI [bridge.rs](../crates/basecraft-gui/src/bridge.rs) `activity_log` using ops `log::activity_text` → human/text opener | Unversioned generated text, replaced by same-session export; no JSON unknown-field contract | Private derived activity text; no golden |
| **AU04** user-selected authored Markdown (including `reports/methods.md`) | GUI [provenance_bridge.rs](../crates/basecraft-gui/src/provenance_bridge.rs) `save_authored` → report/user inspection; Sanger `carry_authored`, provenance packaging retain text | Source-revision HTML comment plus user text, no independent format version; explicitly saving replaces destination; report generation does not regenerate authored text | Public authored document; behavioral authored-text preservation tests, no writer-byte golden |
| **AU05** `hash/<stat-key>.b3`, C cache | IO [cache.rs](../crates/basecraft-io/src/cache.rs) `content_hash_in` → same function | `b3:` text, no envelope/version; best-effort atomic replacement; no extensions; `content_hash` itself now hashes directly | Private disposable cache; no committed cache golden |
| **AU06** `index/<stat-key>.fai2`, C cache | `cache::open_fasta_in`, `FastaIndex::to_bytes` → `open_fasta_in`, `FastaIndex::from_bytes` | Binary `BCFAI` magic with byte version 2; checked decoding or rebuild; `open_fasta`/`index_fasta` do not persist an index | Private disposable cache; in-memory binary round-trip and cache invalidation tests in IO, no committed binary golden |
| **AU07** allocated distance cache, A artifact / explicit engine cache path | [distance/lib.rs](../crates/basecraft-distance/src/lib.rs) `DistanceMatrix::write_identified`, `write_compact` → `DistanceMatrix::read`, `from_compact`; distance wrapper checks supplied identity | `schema: 1`; JSON `DistanceMatrix`/flattened `IdentifiedMatrix` or `BASECRAFT-DISTANCES-1` binary with JSON `CacheHeader`, numeric arrays and checksum; ordinary typed extras dropped, identity is Value; calculation rebuilds | Scientific retained reusable cache, distinct from AU05/AU06; compact/legacy JSON/damage tests in distance lib, no committed original-writer cache golden |
| **AU08** `<distance-result>.run.json` | [distance_plugin.py](../plugins/distance/distance_plugin.py) `invoke`, `recovery_error` retain engine stdout metadata → wrapper `retained_output`, registered artifact consumers, human | Engine main builds Value report, tool `version` is not schema version; failures add `output_status`/`error`; dict retains producer fields, new invocation rebuilds; no complete protocol type | Public analysis evidence, including failed partial collection; distance retention tests exercise it, no golden |
| **AU09** allocated distance bootstrap progress JSON / retained tree collection | [bootstrap.rs](../crates/basecraft-distance/src/bootstrap.rs) `bootstrap` → wrapper/engine consumers and caller monitoring; NEXUS collection containing Newick tree expressions | `BootstrapReport` via `write_json`, no independent schema envelope; progress is overwritten, collection retains completed prefix in `.partial.nex` when support is withheld; wrapper requires `format: "nexus"`; serde fields do not retain arbitrary extras | Progress private working state; collection/report scientific evidence; bootstrap interruption/collision tests, no committed byte golden |
| **AU10** delimitation `result.json` / `<result>.run.json` | [delimitation/main.rs](../crates/basecraft-delimitation/src/main.rs) `main` and [delimitation_plugin.py](../plugins/delimitation/delimitation_plugin.py) `invoke` → wrapper, workflow, `files::load_delimitation` | Protocol `DelimitationResult` (`version: 1`, JobRecord nested), wrapper augments JSON; [delimitation-result.schema.json](delimitation-result.schema.json) covers typed result, not all additions; retained Value additions survive augmentation, typed envelopes drop extras | Public analysis result; engine/plugin behavioral tests, no committed original-writer full report golden; separate from RP07 |
| **AU11** `<result>.haplotypes.json` and `.unique.fasta` | delimitation main `run`, `artifact`, [haplotypes.rs](../crates/basecraft-delimitation/src/haplotypes.rs) `deduplicate` → `AlignmentWorkflow::run` mapping load / `haplotypes::expand` | Local `Haplotypes` JSON, policy string ending `v1` (algorithm policy, no schema version); typed extras dropped, workflow Value mapping read; fresh output | Public scientific input transformation evidence; haplotype tests and workflow tests, no golden |
| **AU12** delimitation progress file | delimitation main `progress` → caller monitoring | Value job/stage IDs, generation, phase, completed/total, no version; overwritten snapshot | Private control state; engine tests, no golden |
| **AU13** allocated BLAST receipt | [blast_plugin.py](../plugins/blast/blast_plugin.py) `submit_prepared`, `write_receipt` → `inspect_retained`, `receipt` | Dict `schema: 1`; selective identity/status checks; inspect returns original dict, submission creates new receipt; no own protocol type | Public remote-submission evidence, must retain uncertain outcome; `test_blast.py`, host `blast_submission.rs`, no golden |
| **AU14** allocated Zenodo selection plan | [zenodo_plugin.py](../plugins/zenodo/zenodo_plugin.py) `prepare`, `save` → `upload`, `load`; runner retains remote plan in EX10 | Dict `schema: 1`, selection hash and file identities/snapshots; unknown fields selectively ignored; fresh prepare rebuilds | Retained input plan; runtime snapshot paths are not portable references; plugin tests, no golden |
| **AU15** allocated Zenodo draft/retry receipt | same module `upload`, `save` → `upload` resume input via `load` | Dict `schema: 1`; status/remote identity/metadata/uploaded members; resume copies selected identity fields into new receipt, does **not** merge unknowns | Public remote-action evidence; no credentials saved; unknown outcome must not be treated as disposable state; plugin/host Zenodo tests, no golden |
| **AU16** Tracy `evidence.json` / draft output and trace-call JSON | [consensus.py](../plugins/tracy-sanger/consensus.py) `merge`, `trace_calls`; [tracy_plugin.py](../plugins/tracy-sanger/tracy_plugin.py) `consensus`, `calls` → docs `SangerDraft::read`, `TraceCalls`, review `from_draft` | DS10 draft shape; native trace JSON is Tracy's format, parsed into typed calls (legacy call forms also supported); raw producer bytes retained separately; no independent Basecraft schema version | Public evidence; `test_consensus.py`, docs Sanger tests and storage tests, AB1 compact fixtures are **inputs**, not draft/review goldens |
| **AU17** allocated `primers.json` | Tracy wrapper `primers`, [primers.py](../plugins/tracy-sanger/primers.py) `detect` → GUI `sanger_work::execute` reads JSON payload for review workflow | Dict primer/mask evidence, no schema version/protocol envelope; Value reader, fresh producer output | Public supporting evidence; primer tests, no golden |
| **AU18** allocated `addition.json` / placement JSON | [add_sequences.py](../plugins/mafft/add_sequences.py) `run_add`; [sanger_place.py](../plugins/mafft/sanger_place.py) `place` → `sanger_work::execute`, docs `SangerPlacement::read`, review storage | Dict inserted rows/mappings or placement evidence; no schema-version envelope; DS11 typed reader drops unknowns, storage retains exact artifact bytes | Public scientific evidence; MAFFT add/place tests, no golden |
| **AU19** IQ-TREE retained `recovery.json` in partial publication recovery directory | [output_contract.py](../plugins/iqtree/output_contract.py) `publish_artifacts` → human/repair inspection (no automatic Basecraft recovery reader) | `{version: 1, artifacts: [...]}` dict; generated once, no rewrite; **not RP01** | Private repair candidate, keep accompanying retained outputs; output-contract failure tests, no golden |
| **AU20** allocated Sanderson report JSON / chronogram | [plugin.py](../plugins/sanderson/plugin.py) `date_tree` → result consumer, artifact verification; native engine response parsed by wrapper | Dict report with `schema: 1` and engine/scientific fields; nested tool version is separate; no protocol schema; wrapper builds fresh report; chronogram Newick | Public scientific output; plugin/native tests, no host-record byte golden |
| **AU21** persisted `snapshot.arrow-pair` | runner `PreparedOperation::prepare`, snapshot bytes from [snapshot.rs](../crates/basecraft-plugin-host/src/snapshot.rs) → artifact digest/retained-input inspection; live plugin reads shared-memory snapshot streams (no standalone durable-pair loader found) | Pair length framing plus Arrow IPC streams, not JSON; schemas/field meaning from shared snapshot contract; no record-version envelope or JSON extension preservation | Retained frozen analysis input; transport tests are not an original-writer golden for this durable container |
| **AU22** `operation-jobs/<id>/query-source.json`, C state | GUI [operations_bridge.rs](../crates/basecraft-gui/src/operations_bridge.rs) `submit_blast` → runner query input loading, ops `blast::Query` preparation | `Vec<Query>`, no outer version; typed query extras dropped; newly allocated file, not rewritten | Private staging input retained for the operation, scientific source bytes frozen by runner; not EX02 request JSON; BLAST/interface tests, no committed writer golden |
| **AU23** `<target>.run.json` on mPTP failure, plus `<target>.mptp-<id>/input.json` | [mptp_adapter.py](../plugins/delimitation/mptp_adapter.py) `invoke` writes failure dict; native `inspect_tree` through `run_native` writes input inspection → adapter reads metrics/tips; human/artifact consumers inspect failure | Failure `{job, command}` has **no outer version** and is not full DelimitationResult; inspection is AU10 typed result; new failure record, no rewrite/extension merge | Public failed-action/retained-tool evidence; adapter tests, no committed byte golden; native result text/stdout/stderr/snapshot are separate producer/standard files |

AU05/AU06 refine LC06; AU08–AU20 and AU23 refine EX16 rather than inventing additional
host records. DS02 also has writers in the delimitation engine and wrapper:
engine `main` writes version 1, wrapper `invoke` recalculates exchange hashes and
rewrites that wrapper from selected fields after publishing SPART. Version 2 is
used by ops path-base-aware writes. These are not interchangeable byte formats.
DS12/DS13's concrete owner is `SangerRun::prepare_with_dependencies`, `parameters`,
`status` (not a type named `ReviewRun`). DS14's owner is
`SangerReviewSession::checkpoint`; `decode` accepts version 1 or 2 and rejects
other review versions. Nested draft/tool versions do not replace that check.

### Boundary exclusions and standard outputs

The audit follows durable writes through ops, plugin-host, docs model codecs,
platform settings, IO, GUI/CLI adapters, bundled Python plugins and native distance
and delimitation engines. It follows serializers to their actual filesystem
callers; serde derives or filename matches alone are not record evidence.
`basecraft-docs` supplies models/validation; the reviewed document writes are
owned by ops or plugins. Public schemas describe payloads as well as records:
Feature, metadata/submission edits, DelimitationRequest and operation contracts
are not each an independently stored record.

* `OperationRequest` is transient at CLI/MCP input but **EX02** once runner writes
  it. Sanger DS12 and native-engine scratch `request.json` are different shapes.
  Sanderson subprocess requests, distance `groups.json`, temporary MAFFT input
  FASTA/map files and Tracy scratch output are invocation scratch, not new host
  provenance envelopes. They become retained evidence only when explicitly copied
  into the run/review artifact set.
* Primary document formats use `files::{write_main,write_alignment,write_tree,
  write_delimitation,write_sidecar}`, IO FASTA/GenBank/GFF3/Newick/SPART codecs and
  corresponding loaders. GFF3 annotation companions are standard text; they must
  not disappear from the audit just because they are not JSON. Metadata TSVs are
  DS07. Standard NEXUS forest retention differs from a single tree JSON sidecar.
* Distance `DistanceMatrix::export_tsv`, group summaries, bootstrap NEXUS collections containing
  Newick tree expressions (including retained `.partial.nex` collections), delimitation native trees/SPART, MAFFT alignments, FastTree/IQ-TREE
  Newick/support/logs, trimAl alignments, clade-alignment FASTA, ORF/pyrodigal
  proposals, table2asn package/report files and Sanderson chronograms are standard
  or producer-specific scientific artifacts under EX16/EX19. ORF/pyrodigal plugin
  proposals travel in InvokeResult; no extra durable proposal JSON writer was
  found. Opaque retained artifact/object bytes have digest verification, not an
  invented Basecraft schema. Engine/tool-specific intermediate files remain tool
  contracts, not an assertion that Basecraft can parse every native output.
  trimAl also writes `trimal-identifiers.json`: [trimal_plugin.py](../plugins/trimal/trimal_plugin.py)
  `trim` writes an unversioned `tN`-to-original-header dict in the allocated metadata
  directory, alongside stderr. The SDK adapter in [plugin.py](../python/src/basecraft/plugin.py)
  supplies that allocation and recursively returns its files as collection artifacts.
  This is retained EX16 producer evidence, not a new host-record family; no
  dedicated mapping reader/rewrite or committed original-writer golden was found.
* GUI figure SVG/PNG exports (`figure_png`, figure bridges) and CLI accession TSV
  exports (`submission::run`, GUI `write_accession_table`) are user exports, not
  state envelopes. Generated schema JSON from CLI schema generation is build
  metadata. MCP replies, Python SDK JSON strings and QML snapshots are transport,
  not records unless a documented writer above persists them.
* `project.toml`, `plugin.toml`, `operations.json`, pinned environment lockfiles,
  `build.json` and sibling installation receipts are **read** inputs to discovery,
  registry, runtime and installation checks. Application crates do not author
  project/build receipts here. Packaging/setup tools have separate build contracts;
  this inventory does not make their metadata into scientific provenance.
* Runner `progress` and `cancel`, engine progress snapshots AU09/AU12, tool
  stdout/stderr, atomic-write temporary files, lockfiles and directory reservations
  are control/scratch bytes. Registered plugins write progress through context;
  GUI `operations_bridge::poll` reads plain progress for task display;
  `delimitation_bridge::poll_job` reads legacy JSON phases. Retaining a run can retain these bytes;
  this does not make their presence proof of successful completion.

## Committed fixture coverage and remaining compatibility evidence

Coverage below is **source-inspected**, not a new passing test receipt. A generated
schema, synthetic test JSON, temporary round-trip or successful workflow is not a
committed original-writer golden. Every RP/DS/EX/LC row is addressed below; AU rows
state their own evidence. Standard/opaque outputs need their format/producer tests
rather than a fabricated common JSON golden.

| IDs | Actual committed evidence and harness | Remaining gap |
|---|---|---|
| RP01–RP02 | `records-v1/recovery.json`, `pending-save.json`, `pending-save-new.json`; ops `tests/record_formats.rs` | Synthetic setup substitutions documented in fixture README; runtime oracle uses actual paths; not arbitrary future-event preservation |
| RP03 | `provenance-v1/floating-metrics-baseline.jsonl`, submission `legacy-history/events.jsonl`; log/replay tests | Legacy log examples are not golden coverage for every checksummed envelope/event/operation variant or future operation |
| RP04, RP09 | `records-v1/integrity-{empty,nonempty}.json`, `binding.json`, `binding-nested.json`; ops log/analysis-store format tests | Presence-only poison reader differs from typed parser; no new automatic repair guarantee |
| RP05, RP12–RP14 | Complete `provenance-v1/gene.fasta.provenance` bundle; [verify_provenance.py](verify_provenance.py), ops provenance tests | Captured bundle/report/hash compatibility evidence, not full extension-safe rewriting or all publication routes |
| RP06 | Analysis-store, evidence/save safety and provenance workflow tests create inputs at runtime | No committed original-writer SubmittedInput golden covering reference/generated/preserved/broken/removed/dependency shapes |
| RP07 | Nine `records-v1/run-*.json`; ops `tests/run_record_formats.rs`, runner `record_format_tests` | Capture origins and runtime oracle limits documented above; not a lifecycle/replay compatibility horizon |
| RP08, RP10 | `records-v1/store*.json`; ops `tests/store_registry_formats.rs` | Empty registry is synthetic missing-file default; local paths are deliberately synthetic |
| RP11 | Export-history/package tests | No committed export-map original-writer golden |
| RP15–RP19 | `document_record` tests and GUI document-record QML tests | No committed complete Report::write_files golden for these distinct export shapes; provenance-v1 is a different writer |
| DS01–DS02 | `files`, tree/Delimitation save, provenance/replay and plugin tests | No committed original-writer tree or v1/v2 delimitation wrapper golden; SPART inputs and workspace schema cover different bytes |
| DS03 | Five `records-v1/link-*.json`; ops `tests/link_record_formats.rs` | Both versions captured; unknown extra cases are share/update captures, not fresh-create merge guarantees |
| DS04–DS05 | `links` partial-publication/recovery tests | No committed repair-snapshot/guard golden, including absent/file/symlink prior entry variants |
| DS06–DS08 | Metadata/submission workflow, unknown-field and replay tests; `fixtures/metadata` and `fixtures/submission` contain import/source inputs | Import spreadsheets/alignments are not output manifest/TSV/submission goldens; nested rewrite preservation remains incomplete |
| DS09–DS11 | Review/storage tests and docs Sanger tests; compact AB1/reference inputs | No committed saved v1/v2 review, embedded evidence, draft or placement original-writer golden |
| DS12–DS14 | `SangerRun`, review checkpoint, history/job and recovery behavioral tests | No committed legacy request/status/recovery-manifest golden; do not conflate with EX02/EX03/EX06 |
| EX01–EX02, EX05, EX07–EX10 | operation-contract/registered-jobs/runner tests use real writes and synthetic plugin fixtures | Protocol schemas and registry-plugin request data are not original-writer descriptor/request/outcome/validated/tuple/stages/query-plan goldens |
| EX03–EX04, EX06 | `records-v1/execution-*.json`, nine `job-index-*.json`; host `runner/record_format_tests.rs` | Synthetic test workflow and index states, not nine actual transitions; fixture substitutions/oracle exceptions documented above |
| EX11–EX14 | registered-jobs publication/retry/cancellation tests | No committed collection intent/export/member receipt golden; cancel is marker bytes |
| EX15–EX16 | Workflow, engine, plugin and format-specific tests | No single producer-wide golden; concrete missing families split as AU07–AU23 |
| EX17–EX19 | submission tests exercise package export/qualification and failure handling | No committed full original-writer package manifest/summary golden; raw external tool output requires its own producer qualification |
| LC01–LC03 | registry/runtime provisioning, integrity and unknown-field tests | No committed InstallationIndex/Runtime original-writer golden; Pixi lock copy/hash evidence is distinct |
| LC04 | lifecycle contact throttling tests | No committed timestamp golden; scalar is not a typed record envelope |
| LC05 | `schemas/preferences.example.toml`; platform `public_example_is_valid`, update/migration tests; GUI real-QSettings migration checks | Hand-authored example, not an original-writer full-byte golden; TOML store merge preserves unknown values, not comments/formatting |
| LC06 | IO cache invalidation and binary round-trip tests (AU05/AU06) | No committed original cache-byte golden; do not mistake reusable scientific AU07 for this cache |
| LC07 | lock contention/save/evidence/settings behavior tests | OS handle files have no data schema/golden requirement |

## Smallest remaining type migrations (proposals only)

These slices can preserve existing bytes before any separately approved format
change. They are **not** approval to add fields, accept unreadable rewrites, discard
recovery, set a compatibility horizon or implement future-proofing. Every slice
needs original-writer captures first, explicit reader/unknown-field decisions,
actual writer byte oracles and independent persistence review. Shared
`protocol/src/records.rs` and schema-generator registration are an unavoidable
coordination point: serialize those edits or assign distinct new record modules;
“nonoverlapping” below refers to the production owner files, not permission to
edit concurrent shared registration hunks.

| Priority / slice | Bounded production ownership | Reuse and compatibility question before implementation |
|---|---|---|
| 1. DS04/DS05 alignment save repair records | `ops/src/links.rs`; new focused fixture test and protocol record module/schemas | Reuse AlignmentLink for intended links and a wire type for PreviousMetadata; recovery arrays retain raw bytes and symlink paths. Two shapes, no new journal. Keep guard's selected-field refusal and no-final-newline bytes; capture all prior-entry variants. Is future extension retention needed on manual repair round-trip? No automatic repair is authorized |
| 2. EX11–EX13 publication receipts | `plugin-host/src/runner.rs` finalization/export helpers; host format tests and separate protocol record module/schemas | Reuse path/digest maps; keep collection intents and member ownership receipts distinct. Preserve presence-only collection intent gating: finalization reconstructs the expected member map from validated artifacts and checks actual membership/bytes/hashes independently, without decoding either intent. Member repair instead parses selected schema/destination/hash/device/inode/size fields and verifies destination bytes. Do not tighten collection-intent decoding or extend member checks to unknown fields during a type-only migration; capture these distinct acceptance rules first |
| 3. RP12/RP13 provenance report wrappers | `ops/src/provenance.rs` `write_reports`; provenance fixtures and new protocol record module/schemas | Reuse Publication and existing paragraph/citation payload shapes where wire-compatible; manifest vs unversioned array stay distinct. Existing bundle is golden seed. Source-map envelope/version would change bytes and needs separate approval; regenerating reports must still preserve authored methods |
| 4. DS12–DS14 Sanger invocation/index/checkpoint | `ops/src/sanger_review/storage.rs`; storage format tests and separate protocol record module/schemas | Reuse existing source/path concepts; do not substitute LocalJobIndex or ExternalExecution shapes. Confirm owner/status/primary-output nulls and absent artifact_run on recovery manifest. Capture failed/interrupted/support_retained and checkpoint writers; preserve both run/index records and separate-write failure semantics |
| 5. LC01 InstallationIndex | `plugin-host/src/registry.rs`; registry format tests and separate protocol record module/schemas | Existing index preserves top-level unknowns but embeds GPL host Runtime. Reuse opaque runtime snapshots or separately coordinate a portable wire contract; protocol cannot depend on host. Defaults/missing schema and invalid future versions must keep current diagnostics |

RP06 SubmittedInput is important but larger: mutations touch input_evidence,
analysis_store, Session, provenance, runner and Sanger adapters and overlap active
save/registry work. Reuse InputReference/FileStamp and the existing ledger; capture
all states and mutation rewrites before scheduling it. DS01/DS02/DS06/DS09 are also
not merely derive moves: outer/nested extension loss, legacy hash/path trust,
relative bases and version 1/2 call sets need bounded compatibility work. Public
schemas for existing payloads do not remove these gaps. AU13/AU15 remote-action
receipts need explicit retry/uncertainty compatibility tests before any migration.

User decisions remain open on readability/editability horizon, new formats,
standard-vs-companion facts, and writer-version/replay treatment. The inventory
records current behavior and recommends ownership only; it approves none of those
decisions and does not mark parent M6 complete.

### Existing Sanger review payloads

The five `sanger-*.schema.json` files describe existing nested wire payloads now
owned by `basecraft_protocol::sanger`: `ReviewChoice`, `MaskSettings`, `ReviewOp`,
`ReviewSettings`, and `ReviewReference`. Existing docs/ops imports are re-exported.
`ReviewSettings` retains its serde defaults and existing threshold validation;
these schemas do not replace runtime validation.

These payloads occur in saved Sanger reviews (v1 single set, v2 retained sets)
and legacy `jobs/<id>/review.sanger-review.json` checkpoints. The writer remains
`basecraft-ops/src/sanger_review/storage.rs`; the reader/replay owner remains
`basecraft-ops/src/sanger_review.rs`. No envelope, field, version, operation,
identity, or undo behavior changed. The enclosing records are not yet owned by
protocol, and these payload schemas are not complete saved-review schemas.

Compatibility fixtures in `testdata/fixtures/sanger-legacy` retain actual legacy
writer bytes, embedded draft hashes, support closure and checkpoint metadata.
V1 outer/evidence/source/support extras survive; v2 outer extras are rejected.
Nested known operation/settings/reference/choice extras are currently dropped,
and unknown operation variants fail. This extraction preserves those limits;
it does not claim lossless unknown-field rewriting or complete Sanger migration.


## Alignment repair and guard producer types (DS04/DS05)

`link_records` describes the existing schema-1 repair snapshot and temporary
unavailable guard. The prior-entry enum retains absent/file-byte-array/symlink-path
representations; intended origin/view remain opaque serialized byte arrays or
explicit null. Both producers retain recursively sorted pretty JSON without a
final newline. The guard's recovery basename retains Serde's native OsStr
representation (`{"Unix": [bytes]}` / `{"Windows": [UTF-16 units]}`), or null,
rather than converting it to a string. The existing unique-file retention and atomic guard writer remain
the publication owners; no repair subsystem or strict production decoder is added.
Unknown-field rewrite/extension policy is not established by these producer types.

Six `records-v1/link-{repair,guard}-{absent,file,symlink}.json` original-writer
captures were frozen before substitution. The unit harness uses test-prepared
staged bytes from existing DS03 captures and real prior filesystem entries, then
calls the unchanged publish producer with a failing data callback. They are not
complete real analysis/Save As journeys. Runtime literal oracles independently
compare complete bytes at the same inputs and allocated recovery basename; frozen
known-shape fixtures round-trip byte for byte through Value sorting. The original
prior-entry wire declaration is frozen independently in the test oracle.

Production guard acceptance remains selected-field JSON inspection: string
`unavailable` refuses the association independently of schema, and string
`recovery` only enriches the diagnostic. Nonstring fields retain existing
fallthrough behavior. Invalid UTF-8 prior symlink paths now return the existing
JSON error before recovery retention, guard installation or main-data replacement,
rather than panicking in the former literal builder. Existing staged snapshots
can already exist and remain retained. No bytes, versions, defaults, unknown-field
policy, history/replay or compatibility horizon change.

### EX11–EX13 producer typing evidence

The `collection-intent`, `collection-export`, `member-export` and
`member-export-empty` records-v1 examples are immutable original-writer captures,
committed before substitution. Real finalization/export helpers used synthetic
validated staged files and a test-prepared registered collection descriptor,
without executing a scientific engine. Absolute `/fixture` paths, random names
and Unix file identity values are raw capture evidence, not portable identity
promises. Runtime original-literal oracles use independent source inputs and
actual promoted-file stat/hash, never identity parsed from the new receipt.
Known-shape byte round trips retain schema 1, required fields, path strings,
recursive Value ordering, full-width integers and one final LF.

Production readers are unchanged: EX11/12 only require intent presence during
repair and independently verify collection membership/bytes. Malformed/empty or
wrong-schema/destination intent contents still satisfy that presence gate. EX13
selects schema/destination/hash/device/inode/size and checks actual ownership,
links and bytes; unknown fields remain ignored. These types are not strict
production decoders or extension-preserving repair interfaces. Fallible producer
serialization now returns the existing host JSON error for unrepresentable native
paths at the original publication stages; the separate member-reader destination
comparison is outside this correction. Earlier retained publication and stale
intent bytes/temporary cleanup retain their existing behavior.
