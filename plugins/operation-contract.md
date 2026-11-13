# Operation contracts

A plugin declares its operations in a JSON contract. The host validates every
request against it, freezes the inputs, allocates a run folder, starts the
plugin, and checks and publishes what it wrote. The GUI, CLI, Python and MCP
all go through this one route. There is one contract revision: package
manifest schema 2, contract schema 2, wire protocol 2.

## Package

A package holds `plugin.toml`, its contract and its entry script:

```toml
schema = 2
id = "org.example.seqtools"
name = "Sequence tools"
version = "1.0.0"
contract = "operations.json"
[entry]
operation = "python:plugin.py"
```

Any other manifest schema is refused. Entry and contract paths are regular
files inside the package, with no `..` and no symbolic links. The whole
package is fingerprinted; packages over 64 MiB, 4096 files or 32 directory
levels are refused.

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

Installing prints this warning on stderr: "Third-party plugins run code with
your user account's permissions. Install only code you trust. You are
responsible for checking that the plugin's code is safe." Installation enables
the package. A disable choice survives updates and reinstallation. Read-only
sessions cannot run operations that declare document or artifact writes. The
host does not sandbox Python code.

Discovery reads the bundled plugin folder (`BASECRAFT_PLUGINS` overrides it),
the managed plugin folder in the user's data directory and registered editable
folders. It never imports an entry script. The host reads the registry once
per process and again only after an install, update, removal or enable change
rewrites the plugin index. After editing an editable package, refresh the
registry (MCP `basecraft.host.registry_refresh.v1`, or start a new process).
Changed package contents invalidate old pins, so a stale editable package
fails with `plugin_changed` rather than running different code.

## Identity

An operation is `plugin_id/local_name@major`. Plugin ids are lower-case
reverse-domain names of at most 80 ASCII bytes; `basecraft.*` is reserved for
bundled packages. Local names start with a letter and use lower-case letters,
digits, `_` and `-`, at most 32 bytes. Versions have three numeric parts with
major 1 to 9999; a change to scientific behaviour or defaults needs a new
major. MCP spells the id `plugin_id.local_name.vMAJOR`.

## Descriptor

```json
{
  "schema": 2,
  "operations": [{
    "name": "count", "version": "1.0.0", "implementation": "count",
    "title": "Count bytes", "description": "Count the submitted bytes.",
    "family": "file_analysis",
    "parameters": {
      "type": "object", "additionalProperties": false,
      "properties": {"threads": {"type": "integer", "minimum": 1}}
    },
    "result": {"type": "object", "properties": {"bytes": {"type": "integer"}}},
    "inputs": {
      "alignment": {"view": "aligned_fasta", "media_type": "text/x-fasta",
                    "min": 1, "max": 1, "alphabets": ["nucleotide"]}
    },
    "outputs": {
      "artifacts": {"media_type": "application/vnd.basecraft.artifact-collection",
                    "min": 1, "max": 1024, "visibility": "result",
                    "collection": true, "primary": "tree.nwk"}
    },
    "effects": {"document_write": false, "artifact_write": true},
    "needs": {"cpu_parameters": ["threads"]}
  }]
}
```

Inputs. A view is `bytes`, `aligned_fasta`, `ungapped_fasta`, `newick` or
`sequence_snapshot`; it names the transformation the host applies when it
freezes the input. An alignment input may list the `alphabets` it accepts
(`dna`, `rna`, `protein`). The alignment's alphabet is what the user or its
companion file stated, or else the host's guess from the residues. The host
refuses any other alignment before the plugin starts, for example "Distance
tree needs a DNA alignment, and proteins.fasta holds protein sequences", and
passes the alphabet of each alignment input to the plugin. `retain: true` keeps the submitted
bytes with the run. `verify: true` checks the input's retained evidence before
the run, as for a reused distance cache.

Outputs. A single-file role has `max: 1`. A collection role is one folder;
`primary` names the member that holds the main result, which the GUI opens and
history reports. `facts` declares what is known about the primary member
(`alphabet`: `dna`, `rna` or `protein`; `branch_lengths`: `substitutions`,
`relative_time` or `{"absolute_time": {"unit": "Ma"}}`); the IQ-TREE, FastTree
and distance trees declare substitution lengths. The host writes the declared
facts, marked as stated, into the primary member's companion file
(`<file>.basecraft.json`), together with the tree's support semantics. `tree_support` says what support values on output trees mean, optionally
only when a parameter has a value. `bootstrap_exports` lets a run export its
`bootstrap-<run>.nex` trees beside the exported tree when
`bootstrap_destination` is `working`. Output roles need `artifact_write`. File
analyses never write documents.

Sequence proposals have `family: "sequence_proposal"`, exactly one
`sequence_snapshot` input and `document_write: true`. They return edits.

Needs list what an operation wants from the host besides its inputs:

- `credentials`: credential name to the environment variable the plugin reads.
- `contact_email`: a declared parameter the host fills from
  `BASECRAFT_CONTACT_EMAIL` when the caller leaves it out.
- `workflow_stages`: stage name to registered operation id, for host workflows.
- `cpu_parameters`: integer parameters that receive the CPU threads granted to
  the run.

Parameters never choose executables, environment, output paths, run ids or
cancellation files; those names are reserved.

## Handler

A Python plugin registers one handler per declared implementation and calls
`run()`:

```python
from pathlib import Path
from basecraft.plugin import Plugin

plugin = Plugin("org.example.seqtools", "1.0.0")

@plugin.operation("count")
def count(parameters, context):
    source = Path(context.input("alignment"))
    Path(context.output("artifacts", "count.txt")).write_text(str(source.stat().st_size))
    return {"data": {"bytes": source.stat().st_size}}

plugin.run()
```

A handler returns a dict. `context` holds what the host allocated, with
absolute paths:

- `input(name)` is the one file bound to `name`; `inputs[name]` lists them all.
- `output(role, member=".")` is the allocated file, or a member inside a
  collection (its parent folders are created).
- `run_id`, `threads`, `cancel_file`, `progress_file`, `cancelled()`.
- `snapshot`: the sequence a sequence proposal edits; `None` otherwise.
- `alphabets[name]`: `dna`, `rna`, `protein` or `unknown` for each alignment
  input. A plugin uses it instead of guessing from the residues.
- `evidence[name]` lists where each submitted input came from;
  `source(name)` adds `path`, a stable copy for provenance.

Write only into allocated outputs. Unless the handler returns `artifacts`
itself, the SDK lists every file left in the allocated outputs. Return `data`
matching the result schema; `message`, `tool` and `execution` are optional.
`basecraft.plugin.local_context(root, inputs, outputs)` builds a context for
calling a handler directly in a plugin's own tests.

A contract may also declare `queries`: questions the plugin answers without
inputs, outputs or a run, such as which external engines are installed.

```json
"queries": [{"name": "engines", "description": "Engines this installation can run.",
             "result": {"type": "object"}}]
```

`@plugin.query(name)` registers a function with no arguments that answers one.
The host refuses an undeclared name and checks the answer against `result`.

## Invocation context

The host sends this with every call:

```json
{
  "schema": 2, "operation": "org.example.seqtools/count@1", "run_id": "01J...",
  "run_root": "/project/.basecraft/analyses-v1/runs/01J...",
  "inputs": {"alignment": ["/tmp/basecraft-operation-x/alignment-0.afa"]},
  "outputs": {"artifacts": "pending/artifacts"},
  "cpu_threads": 4, "cancel_file": "cancel", "progress_file": "progress",
  "input_evidence": {"alignment": [{"original_path": "/project/seqs.fasta",
    "source_hash": "b3:...", "submitted_hash": "b3:...",
    "transformation": "canonical-fasta-v1", "run_directory": "/project/..."}]},
  "alphabets": {"alignment": "dna"}
}
```

Paths inside the run folder are relative to `run_root`; an input outside it is
absolute. Plugins see only absolute paths through the SDK.

## Run folder

Each run keeps:

- `request.json`: the request as accepted, with file inputs relative to the
  run folder.
- `descriptor.json`: the descriptor it ran under.
- `execution.json`: the run record (`RunRecord`, schema 3): operation, package
  version and digests, `environment` (the interpreter version the plugin
  reported at start-up, and for a managed runtime the conda channels,
  dependencies and PyPI dependencies it was solved from, the lock and manifest
  digests, tool versions and executable digests), caller, effective
  parameters, CPU threads, the submitted inputs with hashes and paths relative
  to the folder, captured stdout and stderr, and the tool and command the
  plugin reported. Basecraft's own tools, such as a Sanger stage, write the
  same record with no package digests.
- `runtime.json`: the outcome callers see.

The document's history gets one `tool.run` entry per run. It holds the tool's
own requested parameters and a `run` link (the run's id and its folder,
relative to the folder of the log). History and methods read how the tool ran,
its effective settings, preprocessing and citations, from `execution.json`.

The record holds no absolute path and no interpreter path, so a project can
move with its run folders. `runner::recorded_request(folder)` reads the
request back with its inputs resolved against the folder's new location;
running it again gives the same output hashes.

## JSON Schema profile and limits

Schemas use JSON Schema Draft 2020-12, validated offline. The host accepts
types, properties, `additionalProperties`, `required`, `enum`, `const`,
numeric and length limits, array and object counts, `items`, `prefixItems`,
`contains`, `allOf`, `anyOf`, `oneOf`, `not`, `if`/`then`/`else` and acyclic
local `#/$defs/...` references. It refuses remote or file references, `$id`,
dynamic references, vocabularies, `pattern`, `format` and every other keyword.
Parameter schemas are objects with declared properties and
`additionalProperties: false`. Normalisation fills top-level defaults only and
never coerces types.

| Resource | Limit |
| --- | --- |
| Manifest, contract, schema | 1 MiB each |
| Wire frame and structured value | 8 MiB |
| Schema and value nesting | 32 |
| Expanded schema nodes | 4096 |
| Validation work | 8,000,000 units |
| File descriptors per frame | 16 |
| Operations per contract | 256 |
| Input and output bindings | 64 each |
| Input files per request | 1024 |
| Artifacts | 1024 |

Stdout and stderr are captured separately, 1 MiB each. Connecting and
initialising have 20 seconds. Analyses have no timeout unless
`BASECRAFT_PLUGIN_TIMEOUT_SECONDS` sets one. Cancellation writes the cancel
file, then terminates the plugin's process group.

## Credentials

```json
"needs": {"credentials": {"zenodo_access": "BASECRAFT_ZENODO_TOKEN"}}
```

Environment names match `BASECRAFT_*_TOKEN`; at most eight per operation.
`BASECRAFT_TOKEN` is reserved for the plugin connection. A declaration states
a need. A bundled package receives its declared credentials from the user's
environment. An installed package receives one only through
`SessionPolicy::with_runtime_credential`, which binds the grant to the
operation id, package digest and descriptor digest. Every plugin process
starts with inherited Basecraft credentials removed, then gets only its own
grants. Granted values are redacted from captured output, replies and the run
record. No secret belongs in parameters, settings or provenance.

## Host services

`host` names services the host runs around an operation:

- `species_workspace`: the result holds species hypotheses for a workspace.
- `caller`: `single_trace`, the base caller the Sanger review asks for
  alternative calls. It has one input, its primary output is the Sanger draft,
  and its contract answers the `engine` query with `{"name", "sha256",
  "problem"}`. The host passes `engine_sha256` and `signal_threshold`, reuses a
  retained result for the same read and settings, and checks the draft against
  the read before showing it.
- `remote`: a service that sends data elsewhere. `lifecycle` is
  `external_submission`, `retained_receipt` or `remote_draft`; submissions name
  a `query_preparation` and receipts a `retained_query_transformation`. Only
  bundled packages get the remote lifecycle.

`host_workflow` names a workflow the host runs from `needs.workflow_stages`.
