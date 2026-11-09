# Selected files to a Zenodo draft

Export > Zenodo opens a multi-file picker in the current project. Save edits
first: this submits selected **saved bytes**, not unsaved document state.
Preparation freezes bytes through the shared runner and shows filenames, sizes
and SHA-256 hashes. Later edits do not change that snapshot. Choose files again
to prepare a different selection.

Enter the dataset title, description, creators (one `Family name, Given name` per
line), Zenodo license identifier and file access. An open GenBank submission can
suggest title/creators. Sandbox is the default; production requires selecting it.
Supported: datasets, open/closed files, up to 100 distinct filenames and 50 GB
total. Embargo/restricted access and other resource types require Zenodo's website.
Metadata becomes public under CC0 on publication, including for closed-file
records. Choose the file license deliberately.

Use a token with `deposit:write` for the selected service. Basecraft has no OS
credential store yet. The password field is runtime-only and clears after each
attempt and on close. No token enters parameters, project metadata, settings or
execution/provenance records. Never put a token into request JSON, command
arguments, shell history or a saved script.

Create draft uses Zenodo's deposit and raw object bucket APIs. It **never
publishes**, calls the publish endpoint or treats a reserved DOI as published.
The separate **Review and publish on Zenodo** action opens the draft in your
browser: check the exact files, metadata, license and access there, then use
Zenodo's publication confirmation. The returned integer identifies a deposit,
not a DOI or proof of publication. Account authentication is needed for the link.

The shared task UI shows stages and allows cancellation. Escape/Cancel requests
cancellation; Continue in background closes the dialog while work continues.
Cancellation cannot undo remote changes. Socket operations time out after 30s;
the draft process is bounded to one hour. DNS and in-flight socket operations
can delay cancellation; cancellation takes effect before the next request/chunk.

Each attempt writes a JSON recovery receipt into its normal owned run. It records
selection identity, metadata, service, deposit ID, verified uploads and uncertain
requests. Retry retained draft requires the same selection and metadata, retrieves
that draft and skips matching remote size/MD5. It refuses different bytes,
additional files, published deposits and edited metadata. It never automatically
overwrites or retries HTTP requests. Lost create responses leave
`creation_unknown`: inspect your uploads on Zenodo and reconcile manually;
Basecraft refuses to blindly create another deposit. Local artifact repair never
repeats remote requests. Inspect receipt `status` as well as host job state: a
completed local run may describe a partial/uncertain remote attempt.

## CLI, Python and MCP

Installed operation discovery exposes `basecraft.zenodo/prepare@1` and
`basecraft.zenodo/draft@1`. All interfaces use the same registered host operations,
permissions, jobs, frozen input ledgers and artifacts. No dependency is added;
the adapter uses Python's standard library.

For headless clients, inject `BASECRAFT_ZENODO_TOKEN` through runtime environment
secret handling before starting the CLI or MCP server. Python's
`basecraft.operations` starts that server as a child. Do not put literal tokens in
shell history. The GUI injects its value into the accepted child process only,
using the host's opaque memory-only credential override.

Prepare request (paths relative to the current project):

```json
{"operation":"basecraft.zenodo/prepare@1","inputs":{"files":[{"kind":"file","path":"results.fa"}]},"parameters":{}}
```

Run `basecraft operations run basecraft.zenodo/prepare@1 --project PROJECT
--request prepare.json --json`. The outcome's `data.files` lists retained
`snapshot` paths, names and hashes; the `receipt` artifact is the selection plan.
For draft submission declare those snapshots under `files`, the selection receipt
under `selection`, and provide parameters `title`, `description`, `creators`,
`license`, `access_right` (`open`/`closed`) and `service` (`sandbox`/`production`).
Run `basecraft operations run basecraft.zenodo/draft@1` with that request, or call
`basecraft.operations.run` / `Session.submit` / MCP's registered-operation tools.
To resume, additionally declare the previous attempt's receipt under `resume`.
After cancellation interrupts local artifact finalization, `pending/receipt` in
the outcome's evidence directory retains the attempt; publication-only repair can
retain it without repeating remote requests. No new credential or retry database
is introduced.

Contract: [official Zenodo REST API](https://developers.zenodo.org/), consulted
9 November 2026. Focused tests replace only the HTTPS transport with documented
response fixtures and small synthetic files. Live authenticated sandbox and
production upload remain unvalidated without test credentials. No real user
files were uploaded. Native picker/interaction validation belongs to a later
contained desktop validation checkpoint.
