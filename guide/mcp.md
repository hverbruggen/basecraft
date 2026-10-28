# Connecting an assistant over MCP

`basecraft mcp` is a local Model Context Protocol server (protocol version
2025-11-25) that speaks JSON-RPC over stdin and stdout. An assistant that can
launch local stdio servers can use it to open, read, edit and save project
documents, read their history and methods, and run installed operations. The
Python `Session` class uses the same server (see [scripting.md](scripting.md)).

```sh
basecraft mcp /path/to/project [--allow-writes] [--model-id NAME]
```

Each server process is one headless session for one project folder. Paths
outside that folder are refused. The project defaults to the current
directory.

## What the assistant sees and sends

The server runs on your computer. The model usually does not. Everything a
tool returns, including sequences, file names, history and analysis results,
goes to the assistant, and most assistants send it to their model provider.
Read-only mode limits changes, not what can be read inside the project. Point
the server at a folder that holds only data you are willing to share with that
provider.

The server does not attach to the Basecraft window. It reads files as saved
on disk and keeps its own open documents, edits and undo history. Unsaved
edits in the GUI are invisible to it, and its unsaved edits are invisible to
the GUI. Save in the GUI before asking the assistant to work on a file, and
reopen the file in the GUI after the assistant saves it.

## Read-only by default

Without `--allow-writes` the server refuses `apply`, `undo`, `redo` and `save`,
and it omits operations that declare document or artifact writes. Start it
with `--allow-writes` only when you want the assistant to change files or
write analysis outputs. Each edit is logged and undoable within the session.
Project files change only on `save` or when an operation you run writes its
declared outputs.

`--model-id` names the assistant in the recorded history (default
`mcp-client`). Edits are recorded as agent edits under that name, and
operation runs carry it as initiator. A program that is not an assistant
passes `--client-kind script` (and may name itself with `--client`), so its
edits are recorded as script edits. The Python `Session` does this.

## Claude Code

Add the server for one project folder:

```sh
claude mcp add basecraft -- basecraft mcp /path/to/project --model-id claude-code
```

To share the setup with the project, add `--scope project`. Claude Code then
writes `.mcp.json` in the directory where you run it:

```json
{
  "mcpServers": {
    "basecraft": {
      "command": "basecraft",
      "args": ["mcp", "/path/to/project", "--model-id", "claude-code"]
    }
  }
}
```

Add `"--allow-writes"` to `args` to enable edits. Run `/mcp` inside Claude
Code to check that the server started. Other terminal agents that read an
`mcpServers` block take the same command and arguments.

## Desktop clients

Desktop assistants that support local stdio servers take the same command and
arguments in their configuration file. Use absolute paths for both the
`basecraft` executable and the project folder, because desktop applications
usually do not inherit your shell's `PATH` or working directory. One server
entry serves one project folder; add one entry per project.

## Tools

| Tool | Purpose |
| --- | --- |
| `open` | Open a project file; returns its document ID, kind and version |
| `documents`, `state` | List open documents; show size, version and undo state |
| `read` | Residues `[start, end)` (at most one million, `row` for alignments), tree tips, or a delimitation |
| `apply` | One typed edit, refused if the document version changed since it was read |
| `undo`, `redo`, `save` | Undo groups; save in place or to a new project path |
| `document_record` | History and methods for the current revision, including unsaved edits |
| `<plugin>.<name>.v<major>` | Start an installed operation; returns a job promptly |
| `basecraft.host.jobs_get.v1` | Job status |
| `basecraft.host.jobs_result.v1` | Terminal outcome, without running again |
| `basecraft.host.jobs_cancel.v1` | Cancel a job |
| `basecraft.host.jobs_repair.v1` | Retry publication of a partial result, without running again |
| `basecraft.host.registry_refresh.v1` | Reload installed plugin metadata |

The tool list is paged; clients follow `nextCursor`. Operation tools carry the
operation's input and output schemas. Their arguments and outcomes are the
request and outcome described in [scripting.md](scripting.md). Failures return
`isError: true` with the same structured error codes.

Jobs belong to the server process that started them. Cancelling an MCP request
does not cancel a job it started; `jobs_cancel` does. When the client
disconnects, the server cancels its unfinished jobs and exits.

An operation tool can read an open document directly: give it as
`{"kind": "document", "doc": "0", "generation": 1, "revision": 3}` with the
values `state` reports. A clean document is read from its saved file; unsaved
edits are submitted as a snapshot of the current revision, and the run appears
in the document's history. A request pinned to an older revision is refused.
