# NCBI BLAST submission

`basecraft.blast/submit@1` sends one HTTPS form POST to NCBI and returns a
submission receipt. It does not wait for search completion, poll, import results,
retry uncertain submissions or launch a browser in headless clients. The GUI
opens the accepted RID URL automatically and retains Copy link/Open again if
launch fails. On Linux, recognized Firefox/Chromium-family desktop identifiers
use the browser's `--new-window` option with separate process arguments. Other
browsers/platforms use the system URL opener with an explicit window/tab limitation
message. A successful launch does not guarantee that browser policy permits a new
window. No shell evaluates result URLs. Submission completion means accepted by NCBI, not finished BLAST.

Use the sequence analysis controls or sequence/annotation context menu. For a
multi-record FASTA opened as rows, use BLAST in the Alignment ribbon/context
menu: all rows, selected whole rows, or selected rows/columns. Unsaved changes
are included. The GUI suggests blastn or blastp using document molecule metadata,
then residue letters, and offers blastx/tblastn explicitly. Ambiguous unlabelled
input defaults to nucleotide; check the program before searching. Mixed alphabets
must be separated. The selected program determines query alphabet and database;
all query validation still runs through the shared preparation/authorization path.
Search prepares queries locally and submits once. Oversized queries require a
second explicit truncation decision with per-record details.
Whole-sequence FASTA input includes every record, not just the displayed one.
Annotation extraction shares Copy/Extract's biological segment order and reverse
complement logic. Mixed-strand annotations are rejected explicitly.

Queries use nucleotide nt/blastn, nucleotide nr/blastx, protein nr/blastp or
protein nt/tblastn. nt requests set `NO_DATABASE_OVERRIDE=true`; this is an
experimental NCBI option. The receipt records the requested database, not an
invented effective database version. Check NCBI's page for the effective database.
E-value defaults to 0.05; maximum hits to 100. Scoring fields are explicit in
`request_fields`. Megablast is available for nucleotide/nt only.

Set `BASECRAFT_CONTACT_EMAIL` in your local launch environment, or supply
`contact_email` for a submission. The GUI allows entering the authorized service
contact and hides it under Search options when already configured. NCBI's
[API usage guidance](https://blast.ncbi.nlm.nih.gov/doc/blast-help/developerinfo.html#usage-guidelines)
asks API clients to include email/tool contact parameters; the contact requirement
is retained even though EMAIL is not a required Common URL API search parameter. There is no bundled address and no Git-author/user-account discovery.
The contact is sent to NCBI and retained with local request evidence. An absent
or invalid contact prevents upload. Local configuration and receipt files should
not be committed to a source repository.

## Public operation

The normal CLI, Python operations SDK and generated MCP tool all use the same
registered operation. Inputs are FASTA/GenBank files, or a JSON array of frozen
query records (`name`, `sequence`, `source` coordinate metadata). For selections
and annotations, shared Rust helpers in `basecraft_ops::blast` and existing
Extract operations supply these bytes. Multiple files retain input order and
FASTA files include every record. Query IDs (`query_1`, etc.) are unique even
when source names repeat.

Example request (no contact address in this file):

```json
{
  "operation": "basecraft.blast/submit@1",
  "inputs": {"queries": [{"kind": "file", "path": "queries.fasta"}]},
  "parameters": {"alphabet": "nucleotide", "database": "nt"}
}
```

```sh
basecraft operations run basecraft.blast/submit@1 --project /project --request request.json --json
```

```python
from basecraft import operations
outcome = operations.run(
    "basecraft.blast/submit@1", project="/project",
    inputs={"queries": [{"kind": "file", "path": "queries.fasta"}]},
    parameters={"alphabet": "nucleotide", "database": "nt"},
)
print(outcome["data"]["status"], outcome["data"]["url"])
```

MCP exposes `basecraft.blast.submit.v1` with the same request and job methods.
No additional permission popup is required beyond the operation's normal effects.

## Limits and explicit truncation

Preflight validates all records, including ones proposed for omission. Gaps and
whitespace are removed, residues uppercased; RNA U is preserved. Empty/all-gap
queries and alphabet mismatches fail before upload. Aggregate limits are
1,000,000 nucleotides or 100,000 protein residues. Basecraft also bounds evidence
to 4096 records and 3 MiB of prepared query/mapping; exceeding those local limits
rejects before transport, with no silent record loss.

Oversize rejects by default. The GUI previews original/allowed/submitted totals,
exact retained query/source ranges, shortened records and every omitted record.
The proposed policy preserves order: retain complete records, shorten one to the
remaining budget, then omit subsequent records. The user may Cancel/back or
explicitly proceed with truncation. The original remains untouched. Headless
callers must explicitly set `truncate: true` for the same policy. Optional
`approval_hash` binds approval to all original query bytes, names, mapping and
search settings; changing source/settings invalidates it. Submitted lengths,
ranges, omission map and explicit choice remain in the immutable preparation and
receipt, alongside hashes of all original input files.

## Failure and recovery

GUI/CLI/MCP contacts are serialized with an application-state filesystem lock and
at least ten seconds between reservations. There is no background resubmission.
The entire POST (DNS/TLS/upload/response) has a 30-second wall deadline and a
1-MiB response bound; the host enforces a 35-second invocation bound and owned
process teardown. Cancel before send prevents upload. A cancellation racing with
send, timeout, process crash or lost response cannot prove remote cancellation:
status is unknown unless a validated retained receipt proves acceptance.

The adapter writes the allocated receipt as soon as a response is parsed. The
host validates schema, allocated path and frozen query identity, then retains
data before finalization. Accepted RID plus late cancellation/local publication
failure is `partial`; explicit existing job repair publishes retained bytes
without repeating the POST. Keep the evidence directory on any failure. Abrupt
shutdown can leave a receipt without final publication. No automatic crash
resume or new history index is introduced.

After restart, `basecraft.blast/receipt@1` reads/verifies `receipt` and `query`
file inputs against the retained allocated receipt and `query-preparation.json`.
It returns the retained RID/URL without any network call. It does not recreate
job ownership or pretend publication completed. Within the original session,
use the existing job repair operation for partial publication. RID URLs are
temporary links; permanent results must be downloaded in NCBI's browser page.

The receipt reader requires the original query-preparation artifact directory,
not an arbitrary unbound pair of JSON files. Host preparation verifies the shared
input ledger and retained query hash; published receipts also use the shared
artifact/provenance verifier and output identity. Upstream changes are retained
as diagnostics. Domain checks validate schema 1, query SHA-256, approval identity,
full plan equality and the NCBI RID URL. Generic frame/schema/file bounds still
apply. No remote submission occurs. An explicitly invoked receipt read creates
the normal local registered-operation/input bookkeeping in application state;
it does not recreate, poll or resume the original remote search or job.

Service references: [URL API](https://blast.ncbi.nlm.nih.gov/doc/blast-help/urlapi.html),
[usage guidelines](https://blast.ncbi.nlm.nih.gov/doc/blast-help/developerinfo.html),
[aggregate query limits](https://support.nlm.nih.gov/knowledgebase/article/KA-03289/en-us).
