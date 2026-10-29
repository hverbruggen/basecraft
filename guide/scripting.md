# Scripting Basecraft

The `basecraft` command-line tool and the Python package both run the same
operations as the desktop application. Python is a thin client over the CLI: it
starts `basecraft` processes and reads their JSON. It does not load the Rust
core into Python, and it does not interpret schemas, defaults or results.

Neither route attaches to a running Basecraft window. A script works on files
in a project folder. Edits that are open but unsaved in the GUI are invisible
to it.

Set `BASECRAFT` to the CLI executable if `basecraft` is not on `PATH`. The
Python package needs the matching CLI build.

## Installed operations from the shell

Analyses provided by plugins are installed operations. Each has an ID of the
form `plugin/name@major`, for example `org.example.registry-fixture/count@1`.

```sh
basecraft operations list --json
basecraft operations describe org.example.registry-fixture/count@1 --json
```

`list` returns the registry generation, every operation descriptor and any
package diagnostics. `describe` returns one descriptor with its parameter,
input, output and result schemas, its pins and whether its package is enabled.
Neither command runs plugin code.

To run an operation, write the request to a file:

```json
{
  "operation": "org.example.registry-fixture/count@1",
  "inputs": {"sources": [{"kind": "file", "path": "a.txt"},
                         {"kind": "file", "path": "b.txt"}]},
  "parameters": {"mode": "normal"},
  "destinations": {"report": "counts.json"}
}
```

```sh
basecraft operations run org.example.registry-fixture/count@1 \
    --project /path/to/project --request request.json --json
```

`operations run` waits in the foreground and prints one JSON outcome. Ctrl-C
cancels the job. Paths in `inputs` and `destinations` are relative to the
project folder and must stay inside it. Destinations must not exist yet.
`parameters` holds only the operation's declared settings. Unknown settings are
an error, and the host fills declared defaults. Optional fields:

- `pins` (from `describe`) refuses to run if the installed package has changed.
- `request_key` makes a submission idempotent within one session: the same key
  with the same request returns the existing job, and the same key with a
  different request is an error.
- `target` names a document that records the run in its history and receives
  its results, when it is not an input. A species workspace analysing a tree
  or alignment it references is the main case: give the tree or alignment as
  the input and the workspace as `{"kind": "file", "path": "species.spart"}`.
  The new hypotheses are added and the workspace is saved, unless it changed
  while the run worked.

A destination key is an output role, or `role/member` to export one file of
a result collection, such as `"artifacts/tree.nwk": "inferred.nwk"`. Results
are always retained in the project's hidden store; destinations add visible
copies. The request schema is
`schemas/operation-request.schema.json`, and the plugin-facing rules are in
[plugins/operation-contract.md](../plugins/operation-contract.md).

## Outcomes and errors

Every route returns the same outcome object
(`schemas/operation-outcome.schema.json`):

| Field | Meaning |
| --- | --- |
| `state` | `queued`, `running`, `cancelling` or `finalizing` while active; then `completed`, `failed`, `cancelled`, `interrupted` or `partial` |
| `run_id` | Job ID, valid only in the session that started it |
| `requested_parameters`, `effective_parameters` | What you sent and what ran after defaults |
| `pins` | Package and contract identity that ran |
| `data` | Structured result, validated against the operation's result schema |
| `artifacts` | Published output files |
| `proposals` | Document edits applied by the operation, if any |
| `warnings`, `errors` | Messages; each error has a `code` and `message` |
| `evidence` | Folder with the retained inputs and run record |

Always check `state`. A command that printed valid JSON can still describe a
failed run. Error codes are `invalid_arguments`, `incompatible_contract`,
`plugin_disabled`, `plugin_unavailable`, `plugin_changed`, `permission_denied`,
`stale_revision`, `output_conflict`, `unsupported_input`,
`plugin_protocol_violation`, `tool_failure`, `cancelled`, `timeout`,
`publication_incomplete` and `evidence_incomplete`.

## Python: one-off operations

```python
from basecraft import operations

operations.describe("org.example.registry-fixture/count@1")

outcome = operations.run(
    "org.example.registry-fixture/count@1", project="/path/to/project",
    inputs={"sources": [{"kind": "file", "path": "a.txt"}]},
    parameters={"mode": "normal"}, destinations={"report": "counts.json"},
)
print(outcome["data"], outcome["evidence"])
```

`operations.list()` and `operations.describe()` call the CLI commands above.
`operations.run()` and `operations.submit()` start a writable session for that
one job, like `operations run`. `submit` returns a handle with `status()`,
`result(timeout=None)`, `cancel()` and `repair()`. A wait timeout raises
`TimeoutError` and leaves the job running. Leaving the `with` block cancels the
job if it has not finished.

A failed, cancelled or partial run raises `basecraft.operations.OperationError`.
Its `code` is the host's first error code, `run_id` the job, and `outcome` the
complete host outcome.

## Python: a session

`basecraft.session.Session` keeps one headless project session open. It starts
`basecraft mcp <project>` and uses the same tools as an external assistant
(see [mcp.md](mcp.md)), so documents, edits and jobs share one session.

```python
from basecraft.session import Session

with Session("/path/to/project", writes=True) as session:
    for item in session.operations():
        print(item["operation"], item["input_schema"]["properties"]["parameters"])

    doc = session.open("gene.fasta")
    print(session.read(doc["document"], 0, 60))
    doc = session.apply(doc["document"], doc["version"], {
        "op": "seq.replace", "params": {"start": 0, "end": 1, "residues": "T"}})
    print(session.document_record(doc["document"])["methods"])
    session.save(doc["document"])

    job = session.submit("org.example.registry-fixture/count@1",
                         inputs={"sources": [{"kind": "file", "path": "gene.fasta"}]},
                         destinations={"report": "count.json"})
    print(job.result(timeout=600)["data"])
```

| Method | Tool | Notes |
| --- | --- | --- |
| `tools()` | `tools/list` | All tool definitions, every page |
| `operations()` | `tools/list` | Operations this session may run, with input and output schemas and pins |
| `refresh()` | `registry_refresh` | Reload installed plugin metadata |
| `open(path)` | `open` | Returns document ID, kind, version and undo state |
| `documents()`, `state(doc)` | `documents`, `state` | |
| `read(doc, start, end, row=None)` | `read` | Residues `[start, end)`, at most one million; `row` for alignments; tips for trees |
| `apply(doc, version, op)` | `apply` | Refused if the document changed since you read `version` |
| `undo(doc)`, `redo(doc)` | `undo`, `redo` | |
| `save(doc, path=None)` | `save` | Optional new path inside the project |
| `document_record(doc)` | `document_record` | History and methods, including unsaved session edits |
| `submit(op, ...)`, `run(op, ...)` | operation tool | Same arguments as `operations.submit` without `project` |
| `call(name, arguments)` | any tool | Returns `structuredContent` |

`writes` defaults to `False`. A read-only session refuses `apply`, `undo`,
`redo`, `save` and operations that declare document or artifact writes, and
`operations()` leaves those operations out. The typed edit schema for `apply`
is the `operation` property of the `apply` tool's input schema in `tools()`.
A session runs at most 128 jobs.

`client` (default `"Python"`) is recorded as the initiator of operation runs,
and edits are recorded as script edits under that name (history shows
`Script: Python`). CLI commands that edit or run analyses record the client
`CLI`. `timeout` (default 60 s) bounds each
request to the server, not the jobs it starts. Closing the session cancels the
jobs it still owns and waits for the server to exit.

Tool failures raise `OperationError` with the host's code. Transport failures,
such as a server that exits, an oversized frame (over 8 MiB) or malformed JSON,
raise `OperationError` with code `plugin_protocol_violation` and the server's
stderr when available.

## Provenance from the shell and Python

These commands read saved files. They do not run tools or write records.

| Command | Purpose |
| --- | --- |
| `basecraft history FILE [--json]` | History for the file's exact current contents |
| `basecraft methods FILE [--json] [--output PATH]` | Methods text; `--output` exports the report |
| `basecraft provenance inspect RESULT` | Recorded inputs, settings and steps of a saved result |
| `basecraft provenance verify RESULT` | Check recorded hashes and ancestry |
| `basecraft provenance methods RESULT --output DIR` | Write the methods reports |
| `basecraft provenance package RESULT --output DIR` | Copy a verified result with its exact inputs |
| `basecraft replay [PROJECT]` | Replay the project's operation log and check every step |
| `basecraft recover SESSION --output DIR` | Rebuild an unsaved session into a new folder |

From Python, `basecraft.records.document_record(path)` and
`basecraft.records.methods(path)` wrap `history --json` for saved files.
Use `Session.document_record` for a document open in a session.

## Sequence extraction and source evidence

Extract zero-based, half-open ranges into a new GenBank file:

```sh
basecraft extract gene.fasta 10 90 --output region.gb
basecraft extract gene.fasta 10 90 --segments '[[10,30],[60,90]]' --reverse --output spliced.gb
```

The CLI, Python and MCP `extract` tool call the same document operation as the
GUI. Splice segments are ordered biologically; reverse complement is applied
after splicing. Python works on the inspected current session revision:

```python
with Session("/path/to/project", writes=True) as session:
    doc = session.open("gene.fasta")
    session.extract(doc["document"], "region.gb", 10, 90,
                    version=doc["version"])
```

Existing headless destinations and stale inspected revisions are refused.
The output keeps the source hash, revision and selection mapping. Clean sources
are referenced; unsaved source revisions retain their generated input bytes.
`provenance verify` diagnoses changed or unavailable references, and
`provenance package` materializes the exact available source for sharing.

Registered `basecraft.clade-alignment/extract@1` keeps selected `taxa` and leaves
alignment columns unchanged. Its optional `selection_tree` input retains the
tree that guided selection; the GUI supplies its exact current document
revision, including unsaved edits. The alignment remains a separate input.
