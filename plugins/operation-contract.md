# Versioned operation contracts

This describes the schema-2 metadata/SDK foundation. Static validation, local
installation and discovery are implemented. Generic execution, API/MCP job
adapters, proposal application and bundled migration are subsequent phases;
installation alone does not yet expose a runnable generated MCP tool. Existing
schema-1 commands keep their explicit routes.

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
Interpreters, SDK and external engines remain runtime prerequisites; discovery
reports runtime as `unverified`, never as a successful engine check. Environment
metadata does not install dependencies. Current subprocess support is Unix-only.

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

This is a contract example, not an available `operations run` command yet.
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
paths. Generic strict atomic proposal application and stale-revision handling
are not delivered by this metadata/SDK foundation. Authors must not assume that
legacy remapping/dropped-edit behavior provides those guarantees.

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
Bulk file/Arrow data is separate from bounded JSON frames. Runtime result counts,
artifact publication, process output quotas and cancellation enforcement are
remaining runner work; constants alone do not establish those guarantees.
Connection waits are bounded at 20 seconds and initialize reads use a 20-second
idle timeout; shutdown reads/writes use a 500 ms idle timeout,
followed by a 500 ms exit wait and forced direct-child kill/reap. Invocation has
no arbitrary short analysis timeout. Full process-tree cancellation and job
ownership remain P3 work. Native MCP Tasks and GUI attachment are not advertised.
