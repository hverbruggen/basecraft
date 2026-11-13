# Saved provenance and recovery v1

`provenance-publication.schema.json` defines publication records. Metadata is JSON,
not a database. `RESULT.provenance/manifest.json` atomically selects the current
publication; `records/ID.json` keeps immutable publication records. Parent IDs
establish ancestry; different revision IDs may have identical content hashes.
`artifacts/HEX` stores exact original/saved bytes addressed by BLAKE3 (`b3:HEX`).
An explicit project (`.basecraft/project.toml`) stores each result's bundle in
`.basecraft/provenance/BLAKE3(relative-result-path)/` instead of a companion.
Local-only policy stores the bundle under application state `provenance/` and
is not portable until explicitly packaged. None creates no permanent lineage.

Each publication contains exact typed operations, full events including undo/redo,
and effective edits contributing to the saved revision. Imported pre-Basecraft
history is explicitly unknown. Independent completeness dimensions describe known
ancestry, input availability, execution specification and replay verification.
A hash check does not set replay verification or prove authorship.

`reports/` contains `history.md`, `methods.generated.md`, `parameters.tsv`,
`source-map.json` and a frozen report manifest (template version 1). Generated
prose describes typed edits factually; it does not invent trimming rationales,
earlier laboratory methods, or unknown tool arguments/versions. User-authored
`methods.md` is never overwritten. Tool-specific methods/citation templates and
validated RO-Crate export remain separate future interfaces.

CLI: `basecraft provenance inspect|verify RESULT`, `basecraft provenance methods
RESULT --output DIRECTORY`, `basecraft provenance package RESULT --output DIRECTORY`.
The package operation requires all recorded input/output bytes to verify and copies
live sidecars as well as the bundle into a new destination. It executes nothing and
makes no claim that unavailable external software can reproduce the result.
`verify_provenance.py` is an independent Python reader; metadata inspection uses
only the standard library. Full hash verification additionally uses the open-source
`blake3` package. No Basecraft process, network connection or database is required.

Opening is read-only. Recoverable sessions promote on first mutation/explicit
execution into `.basecraft/recovery/SESSION/` of the project, or beside a loose file. The baseline and retained
objects precede edits. Each UTF-8 JSONL event has `_sequence` and `_checksum`, whose
value hashes the compact, key-sorted JSON event without framing fields. One writer
owns each unique stream, rotated at 8 MiB. Entries are synced before acceptance;
failed append rolls back its own tail, and uncertain rollback poisons the writer.
Legacy unframed events remain readable. Strict readers reject gaps/corruption.
`log::verified_prefix` returns the verified byte boundary without changing a torn
or corrupt tail; it never skips a malformed middle event.

`basecraft recover SESSION --output NEW_DIRECTORY` reconstructs typed operations
and undo/redo from retained originals in a staged destination, validates states,
and never runs external tools or overwrites input files. A dirty session survives
clean process exit; completely undone or successfully saved clean sessions remove
local recovery. Journals with uncertain durability require inspection instead of
automatic recovery. Recovery-disabled sessions keep temporary retention in memory.
The current exact-retention limit is 100 MiB per input; larger inputs are rejected
before mutation rather than silently claiming recovery/reproducibility.

Save holds a cooperating-process target lock, compares accepted source/sidecar
hashes and detects newly appeared sidecars. `pending-save.json` records expected
outputs before replacement. Data, annotations and provenance are separate atomic
writes; a completed data replacement with a failed ancillary write reports
`data saved; provenance or sidecar incomplete` and retains pending recovery.
Non-cooperating applications can still race the final filesystem replacement;
network/cloud synchronization is not a distributed transaction protocol.

Jobs persist `.basecraft/jobs/RUN/manifest.json` (of the run's project, or the
`.basecraft` that holds the run) plus `input.snapshot` before
launch and use that retained snapshot as the tool input. Terminal status, submitted
document generation, effective parameters, plugin version, observed tool identity,
CPU allocation, output hashes and diagnostics remain inspectable after temporary
worker cleanup. A running record left by a dead owner is interrupted, never marked
successful. Executable argv and environment lock remain explicitly unavailable in
this format until public plugin execution metadata supplies them.


## Current-document reports (template version 2)

`Session::document_record` queries a frozen current revision without writing files.
The public JSON report returned by `basecraft history --json`, MCP `document_record`
and the GUI includes `schema_version`, `template_version`, `document`, `revision`,
`unsaved`, `warnings`, readable `history` rows, `paragraphs`, `methods`, `citations`,
`history_text` and a serialized `raw` record. Paragraph `activity_ids` refer to
history row IDs; `parameters` preserves the exact operation values. Raw operation
coordinates are zero-based half-open; readable summaries use one-based positions.
The current session revision identifier is distinct from a saved publication ID.

Exports include `methods.generated.md`, `history.md`, `parameters.tsv`,
`citations.json`, `source-map.json`, `record.json`, `report.json` and `manifest.json`.
The manifest pins the report revision, unsaved-state flag and template version.
These are separate atomic writes; an export failure may leave a partial report.
User-authored `methods.md` is not generated or overwritten. Explicit authored-draft
saves include a source-revision comment.

Plugin `ExecutionInfo.methods` optionally supplies a frozen `description` and
`citations` list alongside the existing `methods_template` and resolved parameters.
Old plugins remain readable without those fields. Reports never look up software
versions or citations from the network or a currently installed plugin.

Retained job manifests additionally include `owner_instance` and the submitted
`source_record`. Process identity identifies current jobs, not scientific authorship.
Portable publications remain v1; report generation v2 does not rewrite old records.

## Large-file recovery references

Recovery manifests may include `references`, keyed by the original BLAKE3 hash.
Each value uses the existing `InputReference` record (path relative to the recovery
folder, file stamp, and hash). This optional field is omitted for small inputs;
older manifests and operation logs remain readable without migration.

Files above the 100 MiB copy threshold can be edited. Their bytes are not copied
at open or first edit. Crash recovery requires the referenced original to retain
its recorded contents; a missing or changed original is reported without
publishing partial recovered output. Before Save replaces a referenced original,
Basecraft retains its inode under `.basecraft/recovery-references/<session>/`
beside the source, then atomically updates the manifest. This is a hard link, not
a byte copy, and keeps pre-save undo and replay possible. The link is retained as
provenance evidence; there is no automatic retention expiry. Filesystems that
cannot create hard links report the failure before the original is replaced.

Small originals and companion files keep their exact private recovery copies.
These copies and the recovery manifest are prepared while opening, before the
editor becomes ready. First-edit promotion uses renames, then the Open baseline
and edit are appended as one durable journal batch.
