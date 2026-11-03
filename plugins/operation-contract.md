# Versioned operation contracts

Schema-2 contracts drive discovery, validation and owned execution through the
CLI, Python, MCP and the installed-operation picker. Existing schema-1 packages
retain explicit legacy routes; their forms do not create typed operations.
The implementation requires exact-source validation before release; the examples
below describe the source interfaces, not a claim about an older installed build.

## Package and installation

A package contains `plugin.toml`, a local JSON contract and its entry script:

```toml
schema = 2
id = "org.example.seqtools"
name = "Sequence tools"
version = "1.0.0"
contract = "operations.json"
[entry]
operation = "python:plugin.py"
```

Schema 2 must omit the schema-1 `operations` list. The contract is the sole
operation authority. Entry/contract paths are regular files within the package,
without traversal or symbolic links. The complete package is fingerprinted;
packages over 64 MiB, 4096 files or 32 directory levels are rejected. Scientific
input files are separate bindings, not package contents.

Current commands (all emit JSON on stdout):

```sh
basecraft plugins validate /path/to/package
basecraft plugins install /path/to/package
basecraft plugins list --json
basecraft operations list --json
basecraft operations describe org.example.seqtools/count@1 --json
basecraft plugins disable org.example.seqtools
basecraft plugins enable org.example.seqtools
basecraft plugins remove org.example.seqtools
basecraft plugins register-editable /path/to/development-package
```

Installation and editable registration print this warning on stderr:

> Third-party plugins run code with your user account's permissions. Install
> only code you trust. You are responsible for checking that the plugin's code
> is safe.

Installation enables immediately. There is no acknowledgement flag, separate
MCP grant or repeated run warning. An explicit disable choice survives updates
and removal/reinstallation. Bundled packages start enabled. Session restrictions
still apply: read-only sessions cannot invoke operations declaring document or
artifact writes. The host path/effect policy does not sandbox Python code.

Discovery uses the bundled root (`BASECRAFT_PLUGINS` override), the managed
user-data plugin directory and explicitly registered editable directories. It
never imports entry scripts or searches project directories. `BASECRAFT_PROFILE_DIR`
selects an isolated profile. Discovery does not create missing profile directories.
Discovery reports runtime as `unverified`, never as a successful engine execution check.
Explicit installation provisions optional declared runtimes as described below;
legacy packages retain their existing interpreter/tool requirements. Current subprocess support is Unix-only.

Managed copies are selected by an atomically replaced index, with OS locking.
Each digest/version directory remains available to jobs holding old registry
metadata. Remove only clears active selection; it does not delete retained code
or evidence. Editable packages are fingerprinted and changed contents invalidate
old pins. Duplicate active identities produce diagnostics; directory order does
not choose a winner. Third parties cannot use reserved `basecraft.*` identities.

## Identity and compatibility

An operation is `plugin_id/local_name@major`. Plugin IDs have lower-case
reverse-domain segments, at most 80 ASCII bytes. Local names start with a letter,
contain lower-case letters/digits/underscore/hyphen and are at most 32 bytes.
Semantic versions use three numeric components and major 1–9999. Breaking
scientific behavior or defaults requires a new major. MCP spelling is
`plugin_id.local_name.vMAJOR`, without truncation.

Keep package version, manifest schema (2), contract schema (1), operation version,
wire protocol (1) and MCP version separate. Registry generations and
algorithm-qualified package/descriptor digests pin metadata; they are not trust
certifications or complete dependency-environment fingerprints.

| Manifest/host/SDK | Behavior |
| --- | --- |
| Schema 1, current host, old or new SDK | Existing explicit legacy routes; no inferred typed contract |
| Schema 2, old schema-1 host | Unsupported manifest schema error |
| Schema 2, current host, legacy SDK | Refused: missing `operation-contract-v1` capability |
| Schema 2, current host, context SDK | Identity/version and every implementation binding checked at initialize |
| Claimed capability with missing implementation or wrong identity | Refused before invocation |

The SDK advertises the capability only when context handlers are registered.
Legacy two-argument handlers retain their calling convention. Context calls
cannot silently enter legacy handlers.

## File operation example

`testdata/fixtures/registry-plugin` is a separate conformance package with a
complete multi-input counting contract and context handler. It is not a bundled
scientific method. Its descriptor illustrates:

```json
{
  "schema": 1,
  "operations": [{
    "name": "count", "version": "1.0.0", "implementation": "count",
    "title": "Count bytes", "description": "Count the submitted bytes.",
    "family": "file_analysis",
    "parameters": {
      "type": "object", "additionalProperties": false,
      "properties": {"include_newlines": {"type": "boolean", "default": true}}
    },
    "result": {
      "type": "object", "additionalProperties": false,
      "properties": {"bytes": {"type": "integer", "minimum": 0}},
      "required": ["bytes"]
    },
    "inputs": {
      "sources": {"view": "bytes", "media_type": "application/octet-stream", "min": 1, "max": 8}
    },
    "outputs": {
      "report": {"media_type": "application/json", "min": 1, "max": 1, "visibility": "result"}
    },
    "effects": {"document_write": false, "artifact_write": true}
  }]
}
```

File views are `bytes`, `aligned_fasta`, `ungapped_fasta` or `newick`. They
identify the required host transformation; they do not install arbitrary viewers.
Single-file output roles have maximum 1. Collection roles declare `collection:
true` and a bounded member count. Output bindings require artifact-write effects.
A structured-data-only operation may have no output bindings. File analyses
cannot declare document mutation.

The request type separates inputs, scientific parameters and destinations:

```json
{
  "operation": "org.example.seqtools/count@1",
  "inputs": {"sources": [
    {"kind": "file", "path": "first.txt"},
    {"kind": "file", "path": "second.txt"}
  ]},
  "parameters": {},
  "destinations": {"report": "counts.json"}
}
```

Save the request as `request.json`, using the actual installed operation ID:

```sh
basecraft operations run org.example.seqtools/count@1 --project /path/to/project --request request.json --json
```

Relative bindings resolve beneath the project root. The supplied fixture uses
`org.example.registry-fixture/count@1` and also declares a `mode` parameter.
Parameters never choose executables, environment, output/control paths, job IDs
or host allocation. All file references and destinations go through one policy
resolver. It rejects traversal, symlink ancestors, hardlink input aliases,
duplicate inputs, destination aliases, existing outputs and paths outside the
session root. This is a host-action boundary, not protection against malicious
installed code or arbitrary concurrent filesystem mutation.

## Sequence proposal example

A sequence descriptor has `family: "sequence_proposal"`, exactly one snapshot
binding with minimum/maximum 1, and declared `document_write: true` for edits:

```json
{
  "name": "annotate", "version": "1.0.0", "implementation": "annotate",
  "title": "Annotate a range", "description": "Propose one annotation.",
  "family": "sequence_proposal",
  "parameters": {
    "type": "object", "additionalProperties": false,
    "properties": {"start": {"type": "integer", "minimum": 0},
                   "end": {"type": "integer", "minimum": 1}},
    "required": ["start", "end"]
  },
  "result": {"type": "null"},
  "inputs": {"sequence": {"view": "sequence_snapshot", "media_type": "application/vnd.apache.arrow.stream", "min": 1, "max": 1}},
  "outputs": {},
  "effects": {"document_write": true, "artifact_write": false}
}
```

```python
from basecraft.plugin import Plugin, feature

plugin = Plugin("org.example.seqtools", "1.0.0")

@plugin.context_operation("annotate")
def annotate(snapshot, parameters, context):
    return {"ops": [feature("misc_feature", parameters["start"], parameters["end"])],
            "data": None}

plugin.run()
```

Snapshots retain the existing sealed Arrow residues/features representation.
`context_operation` handlers receive `(snapshot, parameters, context)`.
Context schema 1 includes the public operation/run identity, named frozen input
paths, allocated output paths, CPU allocation and host-owned cancel/progress
paths. The host retains the sealed snapshot and checks the owning session, document
generation and revision before applying the whole typed proposal batch. Invalid
or stale batches fail without applying a subset. Supported edits are sequence
replacement and feature add/remove/update. A successful batch is one undo group;
results never move to the newly selected GUI document. Headless sessions own
independent documents. A file snapshot opens a sequence in that headless session;
inspect the returned document ID and save explicitly through MCP when wanted.
The generic picker currently offers file operations with scalar inputs and flat
settings; sequence forms and headless clients cover proposal operations.

## JSON Schema profile and limits

The host uses JSON Schema Draft 2020-12 with offline `jsonschema` validation.
It supports object/array/scalar types, properties, additionalProperties, required,
enum/const, numeric limits/multipleOf, string length, array length/uniqueItems,
object property counts, items/prefixItems/contains, allOf/anyOf/oneOf/not,
if/then/else and acyclic local `$defs`/`$ref`. Annotation keywords are title,
description, default, examples and `$comment`.

Remote/file references, reference cycles, `$id`, dynamic references, vocabularies,
regex/pattern/format and all other keywords are explicitly rejected. HTTP/file
resolver features are disabled. Only `#/$defs/...` references are accepted in
author schemas; MCP embedding rebases them beneath `#/properties/parameters`.
Annotations and data containing `$ref` are not interpreted as schemas.

Parameter roots require object type, declared properties and
`additionalProperties: false`. Normalization fills only explicit top-level
property defaults, never coerces strings/numbers and never chooses conditional
branches. Defaults validate independently. The normalized object must satisfy
all conditionals; missing required contextual settings fail. The host retains
requested and effective values separately.

The public protocol declares these exact limits:

| Resource | Limit |
| --- | --- |
| Manifest/contract/schema bytes | 1 MiB each |
| Wire frame and structured value bytes | 8 MiB |
| Schema/value nesting | 32 |
| Expanded schema nodes | 4096 |
| Validation work | 8,000,000 estimated units |
| File descriptors per frame/backlog | 16 |
| Operations per contract | 256 |
| Named input/output bindings | 64 each |
| Input files per request | 1024 total |
| Artifacts | 1024 |

Validation estimates expanded schema work multiplied by instance nodes plus
serialized bytes; uniqueItems also multiplies by instance size to bound pairwise
work. Exceeding a limit returns an error; scientific inputs are not truncated.
Bulk file/Arrow data is separate from bounded JSON frames. Runtime artifact count and role cardinality, structured results and all declared
files are checked before publication. Output members must be unique ordinary
files within their allocated role; symlinks, hardlinks, escapes and undeclared
files fail. Stdout and stderr are captured separately, each limited to 1 MiB.
Connections/initialization have a total 20-second bound; writes/shutdown have a
500 ms bound followed by forced process-group termination, leader reaping and
bounded pipe draining. Invocation has no short analysis timeout by default.
A trusted operator may set positive `BASECRAFT_PLUGIN_TIMEOUT_SECONDS`.
Cancellation is cooperative through the allocated cancel file, followed by owned
process-group termination. This is process ownership, not hostile-code isolation.

## Headless clients and owned jobs

```python
from basecraft import operations

metadata = operations.describe("org.example.registry-fixture/count@1")
with operations.submit(
    "org.example.registry-fixture/count@1", project="/path/to/project",
    inputs={"sources": [{"kind": "file", "path": "first.txt"}]},
    parameters={}, destinations={"report": "counts.json"},
) as handle:
    print(handle.status())
    outcome = handle.result(timeout=120)
    print(outcome["data"], outcome["artifacts"])
```

`operations.run` is the foreground convenience form. `OperationError` retains
`code`, `run_id` and the host outcome. A wait timeout leaves the job running;
`handle.cancel()` explicitly cancels it. Each handle runs in its own writable
`basecraft mcp` session; closing the handle cancels an unfinished job and ends
that session. `basecraft.session.Session` runs several jobs and document edits
in one session (see `guide/scripting.md`). Set `BASECRAFT` to the matching CLI
executable when it is not on PATH. The SDK interprets no scientific defaults.

```sh
basecraft mcp /path/to/project --allow-writes
```

MCP stdio targets 2025-11-25. After initialize, page `tools/list` using
`nextCursor`; generated tools use `plugin_id.local_name.vMAJOR`. Arguments are
an OperationRequest; the tool supplies its operation ID when omitted and rejects
a different supplied ID. Calls return accepted jobs promptly in
`structuredContent` and equivalent text JSON. Use
`basecraft.host.jobs_get.v1`, `jobs_result.v1`, `jobs_cancel.v1` and
`jobs_repair.v1`, each taking `{"job":"run_id"}`. Repair retries retained
publication, never execution. `basecraft.host.registry_refresh.v1` refreshes
metadata; changed lists notify `notifications/tools/list_changed`. Old cursors
then fail. Accepted jobs retain their package/contract and session ownership.
Read-only MCP sessions omit writing operations and recheck policy at invocation.
Request cancellation does not cancel an already accepted job; disconnect does.
Native MCP Tasks and live GUI attachment are not advertised.

MCP also offers read-only `provenance_inspect` and `provenance_verify` (a
project path) and `replay`, using the same readers as the CLI commands. An
unknown operation-tool name returns JSON-RPC error `-32602` whose `data` is the
structured error CLI and Python return for that operation ID.

Every analysis, from the GUI, CLI, Python or MCP, runs through this registered
runner (ADR 0022). Inputs are project files or open session documents. A
request may name a `target` document that records the run and receives its
results, and a `role/member` destination that exports one collection member as
a file. CLI `plugin` remains for applying an installed package's sequence
proposals to a file.

CLI/Python and MCP use the same normalized request and terminal outcome. Outcomes
include requested/effective parameters, pins, validated data, published artifacts,
proposal receipt, warnings, errors and evidence path. Errors distinguish invalid
parameters, policy denial, stale metadata/revisions, unavailable packages,
protocol violations, cancellation, engine failure and publication failure.
Inspect `state` even when a foreground command returns valid JSON. Runtime states
are queued/running/cancelling/finalizing followed by completed, failed, cancelled,
interrupted or partial. IDs are usable only within their owning job session;
there is no arbitrary on-disk job lookup or automatic crash resume.

## Results, evidence and recovery

The host freezes inputs before execution. Bytes are unchanged; alignment views
use the existing canonical writer and Newick inputs are parsed. Context contains
named frozen paths, per-input original/submitted hashes and transformation,
allocated role destinations, CPU threads and cancel/progress paths. Authors
must write only allocated outputs and return `data` matching the result schema
and `artifacts` containing role-relative paths (`"."` for a single output).
File operations return no edit proposals. Snapshot plugins receive the existing
sealed Arrow pair via descriptors as well as retained host evidence.

CPU settings declared by `cpu_parameter` or `cpu_parameters` are bound against
the host allocation and validated again. Third-party code uses its declared managed runtime when present, otherwise its legacy
configured interpreter (`BASECRAFT_PYTHON`, otherwise PATH python3). Bundles resolve their
matching plugins, SDK and Python environment beside the executable; environment
metadata alone installs nothing. Managed runtime lock identity is recorded separately
from package digests; legacy package digests do not fingerprint external engines.

All inputs use ordinary shared input ledgers and ancestry. Publication uses the
existing hidden analysis store, output hashes and reference-first export rules.
Single outputs and collection destinations never overwrite existing files.
Collections retain their relative layout. Evidence separates initiating caller
from executor and records package/descriptor identity, effective parameters,
submitted views, output hashes and captured diagnostics. Hidden support files
remain referenced without adding automatic visible original copies.

Finalization can cross multiple file/journal writes. A late failure reports
`partial`, completed writes and retained evidence; these writes are not one
filesystem transaction. Explicit repair uses retained validated bytes, verifies
already published content and refuses collisions. Preserve the run directory
and unrelated outputs when diagnosing failures. Crash records remain inspectable
but are not automatically resumed. Power-loss recovery is distinct from ordinary
I/O rollback of an atomic sequence proposal batch.

## Bundled migration and conformance

Bundled scientific entry points register their unchanged handlers with
`plugin.contract()`. Explicit `legacy_adapter` bindings map public named inputs,
scientific settings and host allocations to those handlers. Forms remain
presentation metadata, not inferred schemas. Diagnostic legacy operations remain
explicit legacy calls. Reserved `basecraft.host` contracts expose existing ASAP
and GMYC alignment workflows; plugins cannot supply arbitrary host workflows.
No generic workflow language or scientific algorithm is introduced.

The independent fixture is under `testdata/fixtures/registry-plugin`. The
source acceptance runner `crates/basecraft-cli/tests/operation_conformance.py`
accepts an already built CLI, matching SDK interpreter and a disposable external
root. It records the unchanged binary checksum, tests install/discovery, all
three interfaces, policy/cancellation and a second operation added by package
update without rebuilding. It never invokes Cargo. Scientific, replay, storage,
QML, packaging and hostile-process regressions remain separate required checks.

## Automatically installed dependencies

An optional runtime table opts a package (schema 1 or 2) into managed provisioning:

```toml
[runtime]
channels = ["conda-forge", "bioconda"]
executables = ["mafft"]

[runtime.dependencies]
python = ">=3.11,<3.14"
mafft = "7.*"

[runtime.pypi-dependencies]
packaging = ">=24"
```

Explicit install/update uses the Pixi executable supplied with Basecraft to solve
and install Conda/native packages and PyPI dependencies automatically. No separate
Conda, global pip, administrator access or manual dependency installation is needed.
`BASECRAFT_PIXI` explicitly overrides the provisioner for developers; otherwise
Basecraft selects adjacent `bin/pixi` before PATH. Missing, non-executable or broken
Pixi, solve/download/install failures and dependency verification failures are
reported by the shared installer. A failed attempt never activates the new package
or changes the previous working runtime. Reinstalling repairs a missing runtime.
The install warning and default-enabled policy are unchanged; an explicit disable
choice survives updates and reinstalls.

Channels are bounded named channels; dependency values are version strings in
Pixi's Conda or PyPI format. No package-provided tasks, shell hooks, local/path/git
requirements or activation scripts are accepted. Unknown runtime fields are errors.
Basecraft always includes Python (default `>=3.11,<3.14`), pyarrow and numpy for its
SDK; constrain these under `runtime.dependencies`, not PyPI. Installation checks
SDK imports, the supported Python version, declared PyPI distributions and every
`runtime.executables` command. List all required native commands there; commands
must exist in the managed environment, rather than accidentally resolving from the
user's PATH. No imports or undeclared tools are inferred. Packages with no runtime
table keep their legacy developer runtime behavior (`BASECRAFT_PYTHON`, otherwise
PATH python3; bundled packages select their matching bundled environment).

Each attempt uses a new prefix in `data/plugin-runtimes/` beside the managed
`data/plugins/` directory, outside immutable package digest trees and outside
source checkouts. User Pixi detached-environment settings are disabled for these
prefixes. A successful prefix retains its generated manifest, `pixi.lock` and
`runtime.json` receipt. The atomic installation index selects both code and runtime;
registry generations and execution evidence include the interpreter, prefix,
manifest digest and resolved lock digest. Accepted jobs retain their previous
selection through updates, disablement and removal. Old environments are retained;
there is no automatic garbage collection. Discovery/list/refresh only read metadata
and verify retained locks; they never solve, install, download or execute Python.
A missing or changed runtime produces a diagnostic and cannot be invoked.

Managed declarations take priority over `BASECRAFT_PYTHON` and ambient Python.
Editable registration does not provision dependencies: install declared packages
to run them; a declared editable runtime without an installed selection is diagnosed.
All process routes prepend the selected interpreter's bin directory to PATH,
including bundled Python. Package `bin/` follows it, before ambient tools. Authors
may ship executable native artifacts there for dependencies unavailable in channels
(subject to licensing and platform compatibility). These are regular package files,
covered by the package fingerprint and executable permissions; they are not downloaded
or built by shell hooks. `runtime.executables` verifies environment-installed tools,
not package-bin artifacts. Do not declare an unavailable channel package and expect
a packaged artifact to override a failed solve. Current process transport is Unix-only.

## Registered workflow stages and distance evidence

Alignment GMYC and ASAP workflows execute registered haplotype, distance-tree
(where needed), and fit sub-runs. Their retained `stages.json` receipts identify
normalized parameters, execution evidence and terminal states. Only the parent
receives the workspace target and applies one result group. Cancellation travels
from that parent to its active registered stage. Stage files and receipts remain
available when a later stage fails; publication repair does not rerun science.

Distance `tree`, `matrix`, and `groups` accept an optional `cache` file binding.
Export a cache with the usual member destination, for example
`"destinations":{"artifacts/distances.tsv.distances.bin":"distances.bin"}`
for a matrix run, then supply `"cache":[{"kind":"file","path":"distances.bin"}]`
in the next run's inputs. The installed-operation picker accepts optional single-file bindings, including
cache inputs; matrix and tree operations can use this there. Every route uses
the same descriptor, frozen input ledger and publication checks. Caches carry the submitted alignment SHA-256,
model, effective minimum overlap and engine executable SHA-256. Older caches
without this producer evidence and mismatched caches are refused; calculate a
fresh cache by omitting the input binding. The engine additionally validates its
matrix schema and semantic alignment identity. Reuse never modifies the supplied
cache. `cache_reused` in the run report distinguishes reuse from calculation.

Distance-tree `bootstrap_destination` retains `metadata` as its default.
Both choices retain the complete bootstrap collection in the run. `working`
also publishes the collection beside the requested tree export, or in the session
root when no tree export is requested, using the shared no-replace publication
and repair receipts. It includes partial collections when support is withheld.
Existing saved form settings keep the same values and need no migration.

Save conflicts group a registered analysis with its dedicated input ledgers,
while preserving every guarded backing decision. Distinct runs remain separate
even if their names and inputs match. Preserve Copy stores matching original bytes
once per analysis in a leaf input ledger; other backing references use existing
verified ledger delegation toward that owner.
Legacy registered ownership is derived only from declared reference-ledger ancestry.
