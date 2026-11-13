# Schemas and on-disk record inventory

JSON Schema files are generated from `basecraft-protocol` by `just schemas`.
Do not edit generated JSON by hand. `just ci` regenerates them and checks for drift.
Journal operations are defined once, in `journal-entry.schema.json`. Records that
carry operations or log entries (publications, document records, plugin results)
show those fields as plain JSON values.
This inventory describes existing writers, including records that do not yet have
protocol types or schemas. It does not establish a compatibility policy.

## The `.basecraft` folder

Everything basecraft records about a user's data sits in a `.basecraft` folder:
the project's, or one beside a loose file. Copying the folder that holds it
copies the records. All records are plain JSON or JSONL; paths in them are
relative and `/`-separated unless a row says otherwise. Basecraft's own
application folders are meant to hold settings, plugins, recent projects and
disposable caches only. Until the open items below close, they still hold:

- LocalOnly history, and the history of a Sanger review whose source folder is
  read-only, in `state/provenance` (j3sxh item 4);
- recovery of a new sequence made without a project, under the scratch folder
  until it is saved (`<scratch>/.basecraft/recovery`).

| Path inside `.basecraft` | What it holds | Format |
|---|---|---|
| `analysis-index/<bucket>/<key>.json` | The analysis index: one analysis store and the files its runs depend on. A project keeps one index, in its own `.basecraft`, with project-relative paths. A file outside the writing session's project is indexed in a `.basecraft` beside it, by name. `<bucket>` is the first two hex digits of the BLAKE3 hash of the path, so a save reads one small folder; `<key>` is a hash of the store's path. Readers take the union of every file in a bucket, so copies a sync tool makes on a conflict still count. `analysis-index.lock` is a lock with no content. | JSON, [analysis-index.schema.json](analysis-index.schema.json), RP10 |
| `analyses-v1/indexed-folders.json` | The index buckets that list this store. A save from a project session asks when a store names a bucket that no longer names it: the project's root store always, and every store in the project once when the whole index is missing. `basecraft records reindex <project>` rebuilds the index. | JSON, [indexed-folders.schema.json](indexed-folders.schema.json), RP10 |
| `analyses-v1/store.json`, `analyses-v1/identity` | Marks the folder as a basecraft analysis store; a random store identity. | JSON (RP08); plain text |
| `analyses-v1/runs/<run>/` | One analysis run: `input.json` (the input it used, RP06), `run.json` (status and result, RP07), `execution.json` and `request.json` (what ran), `records/` (publications, RP05), and kept bytes: `submitted.fasta`, `submitted.spart` or `submitted.spart.xml` for a generated input, `preserved-input` for an original kept before a save replaced it. | JSON and the input's own format |
| `analyses-v1/bindings/<hash>` | Which run produced a result file in this folder. | JSON, RP09 |
| `analyses-v1/trash/` | Runs removed by a save decision, kept for inspection. | As `runs/` |
| `jobs/<run>/manifest.json` | The Tasks view's record of one run (plugin run or Sanger stage): status, owner, source, result and a pointer to the run's own `runtime.json`, which stays authoritative. It sits in the `.basecraft` of the run's project when the writing session's project holds the run, else in the `.basecraft` that holds the run. Paths in it are relative to the folder that holds that `.basecraft`, so a copied or moved project reads its own runs. The Tasks dialog lists the runs of earlier sessions from the chosen project, the recent ones, the scratch folder and the folders of the open files. A run on a loose file shows while that file (or another in its folder) is open; basecraft does not search the disk for others. | JSON, EX06 and DS13 |
| `blast/<id>/query-source.json` | The queries of a BLAST search, kept as the search's input record. | JSON, AU22 |
| `provenance/<hash>/` | The history of a project file, keyed by the hash of its project-relative path (a loose file uses `<file>.provenance/` beside it instead). | RP05, RP12 to RP14 |
| `log/<host>/<month>-<stream>-<segment>.jsonl` | Journal of a session that records into the folder itself (CLI and scripts). | JSONL, RP03 |
| `recovery/<session>/` | Unsaved edits of one open session: `.basecraft/log/<session>/…jsonl` (the edit journal, RP03), `.basecraft/objects/<hash>` (exact bytes the session needs: a checkpoint when an original changed on disk, a new document with no file yet, the input of an analysis started on unsaved edits), `pending-save.json` (a save in progress, RP02) and `recovery-integrity-error.json` (RP04). Removed when the session closes with nothing unsaved. | JSONL and JSON |

Editing needs recovery (decision D13). When the folder of a session cannot
hold `.basecraft` (read-only, or `.basecraft` is a file of another program),
the first edit is refused with the error `read_only_folder`, which names the
folder and says to copy the file to a folder you can write to. The desktop
app offers to save a copy and continues the edit there. A folder that refuses
new files gets no index (basecraft cannot save over a file there either). If
only its `.basecraft` cannot be written, or it is a file of another program,
an analysis of a file there fails and names the folder, and a save of a file
there asks.

## Reading the inventory

Locations: **R** = a session's recovery folder, `.basecraft/recovery/<session>`; **A** = permanent
folder-local `.basecraft/analyses-v1/runs/<id>`; **P** = provenance bundle (beside a
result, in project metadata, or application state according to policy); **J** =
job index, `.basecraft/jobs/<id>` of the run's project or folder; **E** = user-selected export; **C** = application
configuration/cache/state. Filenames are relative to the location in that row.
Identical basenames in different locations are different formats.

Source links identify the owning module; function/type names identify the exact
writer/reader without relying on changing line numbers. `Value` means untyped JSON.
“Kept” means unknown fields survive a deserialize/serialize cycle at the stated
level; it does not mean that a workflow which rebuilds a record merges old fields.
“Dropped” means ordinary serde structs ignore unknown fields when reading and omit
them when writing; “rejected” means `deny_unknown_fields`. Each row describes the current reader and writer; no compatibility with other
Basecraft builds is promised.

## Recovery, journal and provenance

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| **RP01** `recovery.json`: session, source root, baseline event, retention limit, status | R | [session.rs](../crates/basecraft-ops/src/session.rs) `Session::track` | [replay.rs](../crates/basecraft-ops/src/records/replay.rs) `recorded_source_root`; GUI `provenance_bridge::recovery_sessions` | `schema_version: 1` | protocol `records::RecoveryManifest`; [recovery-manifest.schema.json](recovery-manifest.schema.json) | Kept at top level and throughout opaque baseline; readers inspect selected fields |
| **RP02** `pending-save.json`: session, target, before hash or null, ordered expected hashes, status, note | R | `Session::save_checked` | save success removes it; `Session` drop/cleanup preserves recovery if present; user/repair tools inspect it | `schema_version: 1` | protocol `records::PendingSave`; [pending-save.schema.json](pending-save.schema.json) | Kept by type; writer creates a fresh intent, does not merge an earlier intent |
| **RP03** `.basecraft/log/<stream>/<month>-<id>-<segment>.jsonl`: checksummed, sequenced events | R / project | [log.rs](../crates/basecraft-ops/src/log.rs) `LogWriter::append_batch` | `read_all`, `read_readable`, `verified_prefix`, `parse_entry`; replay | `v: 3` (`LOG_VERSION`; 3 adds `proposal.offer` and `proposal.decide`, which builds before it can't read); `_checksum`, `_sequence` envelope | protocol `records::JournalEntry` over `LogEntry` / [journal-entry.schema.json](journal-entry.schema.json) | Envelope verified then stripped; unknown event fields dropped by `LogEntry`; future operation variants rejected |
| **RP04** `recovery-integrity-error.json`: verified length and durability diagnostic | R / project | `LogWriter::append_batch` failed rollback | `replay::recover` refuses poisoned recovery | `schema_version: 1` | protocol `records::RecoveryIntegrityError` / [recovery-integrity-error.schema.json](recovery-integrity-error.schema.json) | Typed round-trip keeps extras; reader checks presence; no rewrite |
| **RP05** `manifest.json`, `records/<publication-id>.json`: publication, ancestry, artifacts, events, effective edits, completeness | P / A | [provenance.rs](../crates/basecraft-ops/src/records/provenance.rs) `publish`, `publish_operation`; session publication routines | `inspect`, `verify`, `package`; `document_record::read_origin`, evidence ancestry readers | `schema_version: 1` | protocol `Publication` / [provenance-publication.schema.json](provenance-publication.schema.json) | Rejected at publication and several nested record types; `InputReference.extra` kept |
| **RP06** `input.json`: source evidence, submitted hash/size, transformation, evidence state, optional ancestry/ledger references | A / P | [input_evidence.rs](../crates/basecraft-ops/src/records/input_evidence.rs) `reference_input_with_dependencies`, `link_generated_input`, `resolve`, `relink`; session/provenance/runner/Sanger adapters | `read_input`, `conflicts_in`, `verified_source`, packaging/ancestry readers | `schema_version: 1` | protocol `input_records::SubmittedInput` (ops public re-export); reuses unchanged `InputReference`, `FileStamp`; [submitted-input.schema.json](submitted-input.schema.json) | Kept via `extra` on input/reference; source stamp has no extra map; `EvidenceState` preserves unknown strings |
| **RP07** `run.json`: id, result name, status and sometimes outputs | A | `ArtifactRun::write_run`, called by save, extraction, the `input_evidence` reference/generated helpers and runner `prepare` | `input_evidence::result_name`, `scan_run`; artifact binding | `schema_version: 1` | protocol `records::AnalysisRun` / [analysis-run.schema.json](analysis-run.schema.json) | Selective `Value` reads unchanged; typed round-trip retains unknown status/extras; outputs omitted or array, result null or string; fresh records |
| **RP08** `store.json`: analysis namespace marker | `.basecraft/analyses-v1` | [analysis_store.rs](../crates/basecraft-ops/src/records/analysis_store.rs) `ArtifactRun::create`, compact JSON plus newline | `ArtifactRun::validate_store` | `format: basecraft.analysis-store`, `schema_version: 1` | protocol `StoreMarker` / [store-marker.schema.json](store-marker.schema.json) | Top-level extras kept by type; validation ignores extras; marker not rewritten |
| **RP09** Result binding (hash-derived filename): run, result filename/hash | `.basecraft/analyses-v1` binding namespace | `ArtifactRun::bind` | `ArtifactRun::for_result` | `schema_version: 1` | protocol `records::ResultBinding` / [result-binding.schema.json](result-binding.schema.json) | Typed round-trip keeps extras; selective reads; fresh writes |
| **RP10** `.basecraft/analysis-index/<bucket>/<key>.json`: one store and the files its runs depend on; `analyses-v1/indexed-folders.json`: the index buckets that list a store | Project root, or beside a file outside the session's project; in each store | [input_evidence/index.rs](../crates/basecraft-ops/src/records/input_evidence/index.rs) `add`, `rebuild` (via `input_evidence::write_json`, `register`, `reindex`) | `index::select` in save checks | `schema_version: 1` | protocol `records::AnalysisIndex`, `records::IndexedFolders` / [analysis-index.schema.json](analysis-index.schema.json), [indexed-folders.schema.json](indexed-folders.schema.json) | Rejected |
| **RP11** `artifacts/<hash>` with logical name `export-map.json`: old/new publication mapping | E packaged P | `input_evidence::export_history` | retained artifact inspection; no dedicated in-app deserializer | `schema_version: 1` | protocol `export_records::ProvenanceExportMap` / `ProvenanceExportMapping`; [provenance-export-map.schema.json](provenance-export-map.schema.json) | Generated artifact; original records also retained, no rewrite reader |
| **RP12** `reports/manifest.json`: publication, template, output hashes, redactions, citations | P / A | `provenance::write_reports` | report consumer / inspection (no dedicated typed in-app reader) | `schema_version: 1`, `template_version: 2` | protocol `report_records::ProvenanceReportManifest` / [provenance-report-manifest.schema.json](provenance-report-manifest.schema.json) | Generated afresh |
| **RP13** `reports/source-map.json`: paragraph/activity array | P / A | `provenance::write_reports` | report consumer / inspection | No version field | protocol `Vec<report_records::ReportSourceMapEntry>` / [report-source-map.schema.json](report-source-map.schema.json) | Generated afresh |
| **RP14** `reports/history.md`, `methods.generated.md`, `parameters.tsv`, bundle `README.md` | P / A | `provenance::write_reports`, `publish` | human/report consumers | Report manifest template version | Markdown / TSV, not JSON records | Not applicable; user-authored `reports/methods.md` is preserved, never regenerated |
| **RP15** `record.json`: document chronology and effective operations | E report folder | [document_record.rs](../crates/basecraft-ops/src/records/document_record.rs) `write_report_files` (`raw`) | `DocumentRecord` consumers / exported evidence inspection | `schema_version: 1` | protocol `report_records::DocumentRecord` / [document-record.schema.json](document-record.schema.json); ops adds runtime origins | Dropped by struct; events retain existing LogEntry behavior |
| **RP16** `report.json`: history, paragraphs, methods, raw record, citations | E report folder | `write_report_files` | report consumer | `schema_version: 1`, `template_version` | protocol `report_records::Report` / [document-report.schema.json](document-report.schema.json) | Dropped by struct |
| **RP17** `manifest.json`: report revision/document/unsaved/template/coordinates | E report folder | `write_report_files` | `check_report_folder` before replacement | `schema_version: 1` | protocol `report_records::ReportExportManifest` / [report-export-manifest.schema.json](report-export-manifest.schema.json) | Selected ownership fields read; regenerated |
| **RP18** `source-map.json`, `citations.json`: paragraph map array / citation string array | E report folder | `write_report_files` | report consumers | No version fields | protocol `Vec<report_records::ReportSourceMapEntry>` / `Vec<String>`; [report-source-map.schema.json](report-source-map.schema.json), [report-citations.schema.json](report-citations.schema.json) | Generated afresh |
| **RP19** `history.md`, `methods.generated.md`, `parameters.tsv` | E report folder | `write_report_files` | report consumers | Companion manifest template version | Markdown / TSV | Generated afresh; ownership check protects unrelated folders/files |

## Document companions and Sanger payloads

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| **DS01** `<tree>.basecraft.json`: exchange/tree hashes, supports, annotations, style | Beside tree / A hidden metadata | [files.rs](../crates/basecraft-ops/src/files.rs) `write_tree_sidecar`, `write_tree_sidecar_at`; plugin-host `file_tool::write_support_sidecar` | `files::load_tree` | `schema_version: 1`; missing/0 legacy accepted with restricted semantics; historical writer omits version | protocol `companion_records::TreeSidecar` / [tree-sidecar.schema.json](tree-sidecar.schema.json) | Outer unknown fields dropped; nested style `extra` kept; legacy backup retained before replacement |
| **DS02** `<species>.basecraft.json`: exchange hash, path base, workspace | Beside SPART / A | `files::write_delimitation_sidecar`, `write_delimitation_sidecar_at`; [delimitation.rs](../crates/basecraft-ops/src/delimitation.rs) artifact writer; session save | `files::load_delimitation` | `version: 1` (relative form) / `2` (path base) | protocol `companion_records::DelimitationSidecar` / [delimitation-sidecar.schema.json](delimitation-sidecar.schema.json) | Outer fields dropped; payload's own serde rules apply |
| **DS03** `<tree>.alignment.json`, hidden origin/view link records | Beside tree / A | [links.rs](../crates/basecraft-ops/src/records/links.rs) `AlignmentLink::create`, `create_referenced`, association/clone/save helpers | `AlignmentLink` loading/verification; `PreparedLinks::prepare` | `schema: 1` snapshot / `2` referenced ledger | protocol `records::AlignmentLink` (ops re-export, behavior via `AlignmentLinkExt`); [alignment-link.schema.json](alignment-link.schema.json) | Top-level `extra` kept |
| **DS04** Retained `*.alignment-recovery.json`: prior/intended origin/view bytes | Beside save target, uniquely retained | `PreparedLinks::publish` | manual repair / diagnostic inspection; no automatic recovery writer | `schema: 1` | protocol `link_records::AlignmentLinkRecovery`; [alignment-link-recovery.schema.json](alignment-link-recovery.schema.json) | Generated snapshot, not rewritten; no extension-preserving repair interface |
| **DS05** Origin/view unavailable guard: diagnostic and recovery filename | Same alignment sidecar locations during partial save | `PreparedLinks::publish` | link loading refuses unusable association | `schema: 1` | protocol `link_records::AlignmentLinkUnavailable`; [alignment-link-unavailable.schema.json](alignment-link-unavailable.schema.json) | Generated guard, not ordinary AlignmentLink; selected-field Value reader unchanged |
| **DS06** `metadata.basecraft.json`: table names/hashes/columns, import settings, partitions | Project metadata folder | [metadata.rs](../crates/basecraft-ops/src/metadata.rs) `encode`, `write` | `metadata::load` | `format`, `version: 1` | protocol `metadata_records::{MetadataManifest, Tables, TableEntry}` / [metadata-manifest.schema.json](metadata-manifest.schema.json) | Manifest top-level extra kept; nested table wrappers drop extras; unknown TSV columns retained by model |
| **DS07** `specimens.tsv`, `taxa.tsv`, `identifications.tsv`, `records.tsv` | Same metadata folder | `metadata::encode`, `create`, session save | `metadata::load` | Via metadata manifest | protocol metadata row/column types; standard TSV | Header/column handling preserves extra data; no JSON schema for TSV bytes |
| **DS08** `*.submission.json` | Project / chosen document path | [submission.rs](../crates/basecraft-ops/src/submission.rs) `encode`, `create`; session save | `submission::load` | `version: 1` | protocol `Submission` / [submission.schema.json](submission.schema.json) | Extra maps on Submission and selected nested types kept; other nested structs have their own rules |
| **DS09** Saved Sanger review | User-selected JSON | shared Session save, Sanger Document writer | SangerReview current envelope | version 3 | protocol SavedCallSets and SavedReview | Current forward scientific decisions; shared Session owns undo/recovery |
| **DS10** `draft.json` and embedded `draft_json`: automatic Sanger draft, trace evidence | A / embedded review | `save_durable` retains exact draft bytes produced by plugin | `SangerReviewSession::from_draft`, `basecraft_docs::sanger::SangerDraft` | No schema version; `version_output` is tool output, not format version | protocol `sanger_records::SangerDraft`; [sanger-draft.schema.json](sanger-draft.schema.json) | Existing typed draft fields drop unknowns on typed reserialization; original JSON retained in review |
| **DS11** Sanger placement artifact (retained JSON) | A / plugin output | Registered plugin produces it; Sanger storage retains/copies it | storage placement verification, docs `SangerPlacement` | Existing producer format; tool `version` is not a schema version | protocol `sanger_records::SangerPlacement`; [sanger-placement.schema.json](sanger-placement.schema.json) | Existing typed payload behavior; no migration here |
| **DS12** Sanger `request.json`, `parameters.json` | A | storage `SangerRun::prepare_with_dependencies`, `parameters` | Sanger provenance/Methods and retained evidence readers | Request has no version; parameters are operation-specific | protocol `sanger_records::SangerRequest` / [sanger-request.schema.json](sanger-request.schema.json); parameters remain operation-specific JSON | Raw parameters retained; not rewritten through a common schema |
| **DS13** Sanger `execution.json` and J `manifest.json`: status, owner, source, result/diagnostic, run path | A / J | storage `SangerRun::status` | `records::jobs`, Sanger resume/storage readers | `schema_version: 1` | protocol `sanger_records::SangerJobRecord` / [sanger-job-record.schema.json](sanger-job-record.schema.json) | Selective reads; status updates build fresh record |
| **DS14** Retired Sanger job checkpoint | — | — | Removed; recovery uses RP01 session journals | — | — | No legacy reader |

## Registered operations, jobs and exports

| Record / shape | Location | Writer | Reader | Version on disk | Current type / generated schema | Unknown fields |
|---|---|---|---|---|---|---|
| **EX01** `descriptor.json`: accepted operation descriptor | A | [runner.rs](../crates/basecraft-plugin-host/src/runner.rs) `PreparedOperation::prepare` | repair, `document_record::task_primary_output` | Descriptor semantic `version` string (not a schema-version field) | protocol `OperationDescriptor` within [operation-contract.schema.json](operation-contract.schema.json) | Descriptor rejects unknown fields; parameters schema is open JSON |
| **EX02** `request.json`: accepted registered request | A | `PreparedOperation::prepare` | repair/retained run inspection; submission qualification | `schema: 1` | protocol `OperationRequest` / [operation-request.schema.json](operation-request.schema.json) | Request envelope rejects unknowns; parameter object preserved |
| **EX03** `execution.json`: plugin identity/pins/runtime/context, initiator/executor, requested/effective parameters, tool/process evidence | A | `PreparedOperation::execute` | `verify_completed_result`, `record_document_run`, submission qualification | `schema: 1` | protocol `records::ExternalExecution` / [external-execution.schema.json](external-execution.schema.json) | Unchanged Value readers; typed round-trip retains nested snapshot extras; new execution builds fresh record |
| **EX04** Host-workflow `execution.json`: workflow/initiator/executor/pins/parameters | A | `PreparedOperation::execute_workflow` | same execution-evidence readers | `schema: 1` | protocol `records::HostWorkflowExecution` / [host-workflow-execution.schema.json](host-workflow-execution.schema.json); distinct external shape | Selective reads unchanged; typed round-trip retains extras; fresh record |
| **EX05** `runtime.json`: operation lifecycle/outcome/validated artifacts/errors/pins | A | `PreparedOperation::persist` (jobs drive transitions) | `lookup_completed_result`, `jobs` repair, `document_record::reconcile_task_receipt` | `schema: 1` | protocol `OperationOutcome` / [operation-outcome.schema.json](operation-outcome.schema.json) | Outcome/error/artifact structs drop unknowns; nested pins reject them; JSON data/parameters retained |
| **EX06** `manifest.json`: lightweight job index with runtime receipt pointer | J | `PreparedOperation::write_task_index` | `records::jobs`, receipt reconciliation | `schema_version: 1` | protocol `records::LocalJobIndex` / [local-job-index.schema.json](local-job-index.schema.json); **not** `JobRecord` | Unchanged selected-field reads; typed round-trip retains extras; rebuilt index, validated runtime receipt authoritative |
| **EX07** `validated.json`: plugin's accepted invocation result | A | `PreparedOperation::validate_result`; [jobs.rs](../crates/basecraft-plugin-host/src/jobs.rs) output validation | finalization/repair | No schema version in current `InvokeResult` | protocol plugin `InvokeResult` / [plugin-invoke-result.schema.json](plugin-invoke-result.schema.json) | Unknown typed fields dropped; plugin data JSON retained |
| **EX08** `validated-artifacts.json`: validated artifact array | A | Same output-validation paths | finalization/repair | No outer version; run contract supplies context | `Vec<(String, PathBuf, String)>` / [validated-artifacts.schema.json](validated-artifacts.schema.json) | Tuple length/types checked; no object extras |
| **EX09** `stages.json`: child operation outcome array | A workflow | [delimitation_pipeline.rs](../crates/basecraft-plugin-host/src/delimitation_pipeline.rs) `AlignmentWorkflow::persist` | runner workflow repair/reconciliation | No outer version; each outcome `schema: 1` | `Vec<OperationOutcome>` / [workflow-stages.schema.json](workflow-stages.schema.json) | Same as outcome |
| **EX10** `query-preparation.json`: query/remote submission plan | A | `PreparedOperation::prepare` | runner lifecycle validation/receipt handling | BLAST plan's existing `schema: 1`; operation-dependent plan payload | protocol `query_records::QueryPlan` / [query-preparation.schema.json](query-preparation.schema.json); remote plan remains producer-owned JSON | BLAST plan structs drop extras; raw remote plan uses Value |
| **EX11** `collection-<role>-intent.json`: expected member map | A | `PreparedOperation::finalize` | finalizer checks intent presence; `verify_collection` checks retained collection against reconstructed expected map | `schema: 1` | protocol `publication_records::CollectionIntent`; [collection-intent.schema.json](collection-intent.schema.json) | Intent contents are not parsed on retry; created before publication, no general rewrite |
| **EX12** `collection-<role>-export.json`: destination and expected members | A | `PreparedOperation::finalize` | finalizer checks export intent presence; `verify_collection` checks destination against reconstructed expected map | `schema: 1` | protocol `publication_records::CollectionExportIntent`; [collection-export-intent.schema.json](collection-export-intent.schema.json) | Intent contents are not parsed on retry; no general rewrite |
| **EX13** `member-<hash>-export.json`: destination and content hash | A | `export_member` | `verify_member_receipt` parses selected schema/destination/hash/device/inode/size fields and verifies destination bytes | `schema: 1` | protocol `publication_records::MemberExportReceipt`; [member-export-receipt.schema.json](member-export-receipt.schema.json) | Unknown JSON fields ignored by selected-field checks; no general rewrite |
| **EX14** Cancellation flag file | A | `jobs::cancel_job` atomic write | plugin cancellation context / process monitor | No version; marker bytes | Not JSON | Not applicable |
| **EX15** Workflow `fit.json`, unavailable `fit.json.run.json` | A staged/published artifacts | `AlignmentWorkflow::run` copies/augments registered stage output | `files::load_delimitation`, workflow/runner output validation | Report `version: 1` where present; producer-dependent fit payload | Protocol `DelimitationResult`/`JobRecord` for compatible reports; augmented `Value`; [delimitation-result.schema.json](delimitation-result.schema.json), [job-record.schema.json](job-record.schema.json) cover typed forms only | Preserve Value while augmenting; typed reader rules vary |
| **EX16** Plugin scientific outputs: reports, input evidence, collections, tool stdout/stderr and native files | A `pending` / `artifacts`, declared destinations | Registered plugin; runner freezes, validates and publishes exact bytes | Contract validation, format-specific readers, artifact digest verification | Producer/standard-specific; declared in operation output contract | Public `PluginArtifact`, `PublishedArtifact`, `ArtifactRevision`; arbitrary scientific JSON is **not** a host record schema | Opaque artifacts remain byte-preserved; do not impose host record fields |
| **EX17** `package.json`: GenBank submission export manifest | E submission package | `submission::export`, `record_validation` | `read_validation`, package validation/receipt qualification | `schema` format identifier, `version: 1` | protocol `submission_records::SubmissionPackage` / [submission-package.schema.json](submission-package.schema.json) | Existing Value retained while updating validation fields; fresh export rebuilds |
| **EX18** `validation/<input-hash>/summary.json`: qualified table2asn outcome/issues/copies | E submission package | `submission::record_validation` | `read_validation`, package status/readme generation | No version field | protocol `submission_records::ValidationSummary` / [submission-validation.schema.json](submission-validation.schema.json) | Unknown struct fields dropped |
| **EX19** Submission tables, issues, README and standard `.fsa`, `.tbl`, `.src`, `.sbt`, `.sqn`, `.gbf`, raw tool reports | E submission package | `export`, `record_validation`, standard IO writers / tool | package validation, external submitter, human | Standard formats / package manifest | Standard text/binary / typed issue rows, not new JSON formats | Exact file hashes recorded; original tool report bytes retained |

## Related setup/cache records (not portable project provenance)

| Record | Location | Writer / reader | Version; type/schema | Unknown fields |
|---|---|---|---|---|
| **LC01** `plugins-v1.json` | C config | [registry.rs](../crates/basecraft-plugin-host/src/registry.rs) `write_index` / `load_index` | `schema: 1`; protocol `installation_records::InstallationIndex` (host runtime specialization); [plugin-installation-index.schema.json](plugin-installation-index.schema.json) | Current complete index/runtime fields are required; missing version is not defaulted |
| **LC02** Managed runtime `runtime.json` | C plugin runtime directory | [runtime.rs](../crates/basecraft-plugin-host/src/runtime.rs) runtime builder / `Runtime::verify` and registry loading | `schema: 1`; protocol `installation_records::Runtime` (transparent host wrapper); [managed-plugin-runtime.schema.json](managed-plugin-runtime.schema.json); distinct from run outcome | Rejected |
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

### Additional state and concrete producer formats

All JSON rows without an explicit schema link below lack a generated protocol
schema for the **complete disk shape**. “No golden” means no committed
original-writer byte fixture was found, not that there are no behavioral tests.

| ID / file and location | Exact writer → reader | Type/version; extensions and rewrite | Role recommendation; fixture evidence/gap |
|---|---|---|---|
| **AU01** `recent-projects.json`, C state | viewmodel [projects.rs](../crates/basecraft-viewmodel/src/projects.rs) `remember` → `load_history`, read by the window and by the CLI and MCP to name the recent project holding a file (`Project::recent_holding`, D16) | Local `ProjectHistory`, `version: 1`; serde drops extras; remember reloads list then rebuilds envelope; unsupported/corrupt file refused | Private paths/history; `project_history_tests` use temporary files, no golden |
| **AU03** `activity/activity-<started>.txt`, C cache | GUI [bridge.rs](../crates/basecraft-gui/src/bridge.rs) `activity_log` using ops `log::activity_text` → human/text opener | Unversioned generated text, replaced by same-session export; no JSON unknown-field contract | Private derived activity text; no golden |
| **AU04** user-selected authored Markdown (including `reports/methods.md`) | GUI [provenance_bridge.rs](../crates/basecraft-gui/src/provenance_bridge.rs) `save_authored` → report/user inspection; Sanger `carry_authored`, provenance packaging retain text | Source-revision HTML comment plus user text, no independent format version; explicitly saving replaces destination; report generation does not regenerate authored text | Public authored document; behavioral authored-text preservation tests, no writer-byte golden |
| **AU05** `hash/<stat-key>.b3`, C cache | IO [cache.rs](../crates/basecraft-io/src/cache.rs) `content_hash_in` → same function | `b3:` text, no envelope/version; best-effort atomic replacement; no extensions; `content_hash` itself now hashes directly | Private disposable cache; no committed cache golden |
| **AU06** `index/<stat-key>.fai2`, C cache | `cache::open_fasta_in`, `FastaIndex::to_bytes` → `open_fasta_in`, `FastaIndex::from_bytes` | Binary `BCFAI` magic with byte version 2; checked decoding or rebuild; `open_fasta`/`index_fasta` do not persist an index | Private disposable cache; in-memory binary round-trip and cache invalidation tests in IO, no committed binary golden |
| **AU07** allocated distance cache, A artifact / explicit engine cache path | [distance/lib.rs](../crates/basecraft-distance/src/lib.rs) `DistanceMatrix::write_identified`, `write_compact` → `DistanceMatrix::read`, `from_compact`; distance wrapper checks supplied identity | `schema: 1`; JSON `DistanceMatrix`/flattened `IdentifiedMatrix` or `BASECRAFT-DISTANCES-1` binary with JSON `CacheHeader`, numeric arrays and checksum; ordinary typed extras dropped, identity is Value; calculation rebuilds | Protocol `distance_records::{DistanceMatrix, IdentifiedMatrix, CacheHeader}`; generated matrix/header schemas; current JSON and checksummed binary producer goldens |
| **AU08** `<distance-result>.run.json` | [distance_plugin.py](../plugins/distance/distance_plugin.py) `invoke`, `recovery_error` retain engine stdout metadata → wrapper `retained_output`, registered artifact consumers, human | Engine main builds typed DistanceRunReport, tool `version` is not schema version; failures add `output_status`/`error`; dict retains producer fields, new invocation rebuilds; protocol declaration covers success and failure | Protocol `distance_records::DistanceRunReport`; [distance-run-report.schema.json](distance-run-report.schema.json); current producer golden |
| **AU09** allocated distance bootstrap progress JSON / retained tree collection | [bootstrap.rs](../crates/basecraft-distance/src/bootstrap.rs) `bootstrap` → wrapper/engine consumers and caller monitoring; NEXUS collection containing Newick tree expressions | `BootstrapReport` via `write_json`, no independent schema envelope; progress is overwritten, collection retains completed prefix in `.partial.nex` when support is withheld; wrapper requires `format: "nexus"`; serde fields do not retain arbitrary extras | Protocol `distance_records::{BootstrapReport, ReplicateCollection}`; [distance-bootstrap.schema.json](distance-bootstrap.schema.json); current producer golden; NEXUS is a standard-format output |
| **AU10** delimitation `result.json` / `<result>.run.json` | [delimitation/main.rs](../crates/basecraft-delimitation/src/main.rs) `main` and [delimitation_plugin.py](../plugins/delimitation/delimitation_plugin.py) `invoke` → wrapper, workflow, `files::load_delimitation` | Protocol `DelimitationResult` (`version: 1`, JobRecord nested), wrapper augments JSON; [delimitation-result.schema.json](delimitation-result.schema.json) covers typed result, not all additions; retained Value additions survive augmentation, typed envelopes drop extras | Protocol `DelimitationResult` and `producer_records::DelimitationReport` (includes request); [delimitation-report.schema.json](delimitation-report.schema.json); current augmented producer golden |
| **AU11** `<result>.haplotypes.json` and `.unique.fasta` | delimitation main `run`, `artifact`, [haplotypes.rs](../crates/basecraft-delimitation/src/haplotypes.rs) `deduplicate` → `AlignmentWorkflow::run` mapping load / `haplotypes::expand` | Protocol `Haplotypes` JSON, policy string ending `v1` (algorithm policy, no schema version); typed extras dropped, workflow Value mapping read; fresh output | Protocol `producer_records::Haplotypes`; [haplotypes.schema.json](haplotypes.schema.json); current producer golden |
| **AU12** delimitation progress file | delimitation main `progress` → caller monitoring | Typed job/stage IDs, generation, phase, completed/total, no version; overwritten snapshot | Protocol `producer_records::DelimitationProgress`; [delimitation-progress.schema.json](delimitation-progress.schema.json); current producer golden |
| **AU13** allocated BLAST receipt | [blast_plugin.py](../plugins/blast/blast_plugin.py) `submit_prepared`, `write_receipt` → `inspect_retained`, `receipt` | Dict `schema: 1`; selective identity/status checks; inspect returns original dict, submission creates new receipt; protocol BlastReceipt | Protocol `producer_records::BlastReceipt`; [blast-receipt.schema.json](blast-receipt.schema.json); accepted/unknown/cancelled current producer goldens |
| **AU14** allocated Zenodo selection plan | [zenodo_plugin.py](../plugins/zenodo/zenodo_plugin.py) `prepare`, `save` → `upload`, `load`; runner retains remote plan in EX10 | Dict `schema: 1`, selection hash and file identities/snapshots; unknown fields selectively ignored; fresh prepare rebuilds | Retained input plan; runtime snapshot paths are not portable references; protocol `producer_records::ZenodoSelection`; [zenodo-selection.schema.json](zenodo-selection.schema.json); current producer goldens |
| **AU15** allocated Zenodo draft/retry receipt | same module `upload`, `save` → `upload` resume input via `load` | Dict `schema: 1`; status/remote identity/metadata/uploaded members; resume copies selected identity fields into new receipt, does **not** merge unknowns | Public remote-action evidence; no credentials saved; unknown outcome must not be treated as disposable state; protocol `producer_records::ZenodoReceipt`; [zenodo-receipt.schema.json](zenodo-receipt.schema.json); ready/unknown-outcome producer goldens |
| **AU16** Tracy `evidence.json` / draft output and trace-call JSON | [consensus.py](../plugins/tracy-sanger/consensus.py) `merge`, `trace_calls`; [tracy_plugin.py](../plugins/tracy-sanger/tracy_plugin.py) `consensus`, `calls` → docs `SangerDraft::read`, `TraceCalls`, review `from_draft` | DS10 draft shape; native trace JSON is Tracy's format, parsed into typed calls (legacy call forms also supported); raw producer bytes retained separately; no independent Basecraft schema version | Protocol DS10 and current producer golden; AB1 compact fixtures are inputs |
| **AU17** allocated `primers.json` | Tracy wrapper `primers`, [primers.py](../plugins/tracy-sanger/primers.py) `detect` → GUI `sanger_work::execute` reads JSON payload for review workflow | Dict primer/mask evidence, no schema version; typed protocol envelope; Value reader, fresh producer output | Public supporting evidence; protocol `producer_records::PrimerReport`; [primer-report.schema.json](primer-report.schema.json); producer golden |
| **AU18** allocated `addition.json` / placement JSON | [add_sequences.py](../plugins/mafft/add_sequences.py) `run_add`; [sanger_place.py](../plugins/mafft/sanger_place.py) `place` → `sanger_work::execute`, docs `SangerPlacement::read`, review storage | Dict inserted rows/mappings or placement evidence; no schema-version envelope; DS11 typed reader drops unknowns, storage retains exact artifact bytes | Public scientific evidence; protocol `producer_records::MafftAddition` and `SangerPlacement`; generated schemas and producer goldens |
| **AU19** IQ-TREE retained `recovery.json` in partial publication recovery directory | [output_contract.py](../plugins/iqtree/output_contract.py) `publish_artifacts` → human/repair inspection (no automatic Basecraft recovery reader) | `{version: 1, artifacts: [...]}` dict; generated once, no rewrite; **not RP01** | Private repair candidate, keep accompanying retained outputs; protocol `producer_records::IqtreeRecovery`; [iqtree-recovery.schema.json](iqtree-recovery.schema.json); partial-publication producer golden |
| **AU20** allocated Sanderson report JSON / chronogram | [plugin.py](../plugins/sanderson/plugin.py) `date_tree` → result consumer, artifact verification; native engine response parsed by wrapper | Dict report with `schema: 1` and engine/scientific fields; nested tool version is separate; generated protocol schema; wrapper builds fresh report; chronogram Newick | Protocol `sanderson_records::SandersonReport`; [sanderson-report.schema.json](sanderson-report.schema.json); current adapter golden with synthetic native response; solver is unchanged |
| **AU21** persisted `snapshot.arrow-pair` | runner `PreparedOperation::prepare`, snapshot bytes from [snapshot.rs](../crates/basecraft-plugin-host/src/snapshot.rs) → artifact digest/retained-input inspection; live plugin reads shared-memory snapshot streams (no standalone durable-pair loader found) | Pair length framing plus Arrow IPC streams, not JSON; schemas/field meaning from shared snapshot contract; no record-version envelope or JSON extension preservation | Protocol `plugin::ArrowPair` owns the binary framing; [sequence-arrow-pair.schema.json](sequence-arrow-pair.schema.json) describes its members; exact actual-writer binary golden in snapshot tests |
| **AU22** `.basecraft/blast/<id>/query-source.json` in the chosen project or a loose file's folder | ops [blast.rs](../crates/basecraft-ops/src/blast.rs) `record_queries` → runner query input loading, ops `blast::Query` preparation | `Vec<Query>`, no outer version; typed query extras dropped; newly allocated file, not rewritten | Protocol `query_records::Query`; [query-source.schema.json](query-source.schema.json); actual staging writer golden |
| **AU23** `<target>.run.json` on mPTP failure, plus `<target>.mptp-<id>/input.json` | [mptp_adapter.py](../plugins/delimitation/mptp_adapter.py) `invoke` writes failure dict; native `inspect_tree` through `run_native` writes input inspection → adapter reads metrics/tips; human/artifact consumers inspect failure | Failure `{job, command}` has **no outer version** and is not full DelimitationResult; inspection is AU10 typed result; new failure record, no rewrite/extension merge | Protocol `producer_records::MptpFailure`; [mptp-failure.schema.json](mptp-failure.schema.json); current failed-process producer golden; native inspection uses AU10 |

AU05/AU06 refine LC06; AU08–AU20 and AU23 refine EX16 rather than inventing additional
host records. DS02 also has writers in the delimitation engine and wrapper:
engine `main` writes version 1, wrapper `invoke` recalculates exchange hashes and
rewrites that wrapper from selected fields after publishing SPART. Version 2 is
used by ops path-base-aware writes. These are not interchangeable byte formats.
DS12/DS13's concrete owner is `SangerRun::prepare_with_dependencies`, `parameters`,
`status` (not a type named `ReviewRun`). DS14 is retired; recovery uses the shared
Session journal. `decode` accepts version 1 or 2 and rejects
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


## Record and writer checks

`basecraft-protocol/tests/record_formats.rs` maps current `records-v1` fixture
prefixes to their production wire types and checks exact serialization, including
field order, sorting, omitted fields, nulls and trailing newlines. Fingerprint
JSONL examples are checked by the protocol fingerprint test. Current SubmittedInput goldens cover
all eight referenced/generated/preserved/broken/relocated cases.

`producer_formats.rs` checks actual bundled producer output for Sanger draft and
placement, MAFFT addition, primers, BLAST and Zenodo receipts, IQ-TREE recovery,
Sanderson reports, distance/cache/bootstrap outputs, query staging, haplotypes,
delimitation reports/progress and mPTP failures. The binary distance cache also
has an exact producer-byte test in the distance crate. These record types live
in `sanger_records`, `producer_records`, `query_records`, `distance_records` and
`sanderson_records`. Synthetic
transport/native responses exercise real producer writers without contacting
services. Python compact separators are checked exactly.

Basecraft-owned records have no backward or forward compatibility promise.
Journal replay requires the current checksum envelope and compares saved content
and complete document fingerprints; it has no old-writer exemptions or unversioned
sidecar fallback. `Publication` does not carry an unused writer map. Save events also omit the unused writer map.

Standard user formats keep their content. `writer_record_formats.rs` check actual serialized output against version-named
goldens, including filled metadata tables and a filled submission. A standard
format writer change requires an intentional version/golden update. Basecraft
record changes update their type and current golden together; older record
versions need no reader or migration.

## Remaining coverage

The supplemental inventory still lists GUI history, settings and IO text/index formats owned by
other lanes without complete shared protocol/golden coverage.
Their rows remain the checklist; this does not claim all of M6 complete. Arbitrary
operation parameters and third-party outputs remain governed by their operation
contracts. Provenance, links, document records, contributions and replay live in
`basecraft-ops/src/records/`; input evidence, the analysis store and the save guard
move there after the save lane merges. The PRD compatibility-promise item is deferred.

### Saved Sanger reviews after Session integration

The current version-3 `SavedCallSets` envelope covers single and multiple sets.
Session owns typed edits, undo, replay, recovery and guarded save; immutable draft
records are addressed by digest. Current examples are in `testdata/fixtures/sanger-review`.
Old payload checkpoints and old envelope/undo readers are removed.
