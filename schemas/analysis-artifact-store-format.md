# Analysis artifacts and referenced inputs v1

A new MAFFT `align`, trimAl `trim`, or generic alignment-to-tree FileJob reserves
`.basecraft/analyses-v1/runs/RUN-ID/` beside the final primary output. No project
marker is required or created. Other analysis families retain their existing
paths until explicitly adapted. Old companion files are never migrated or removed.

`store.json` identifies `basecraft.analysis-store`, schema version 1. An existing
unmarked, foreign or future namespace is refused. Managed paths reject traversal
and symlink directories. Run IDs are unique ASCII alphanumeric/hyphen components.
`pending/` owns incomplete outputs. The host stages primary and ordinary supporting
outputs here; successful no-clobber hardlink publication preserves primary/component
filenames in the working directory and puts supporting files in the hidden run.
An existing destination is an error. File/directory sync errors can leave partial
publication; sources, diagnostics and already published artifacts remain available.
There is no cross-file transaction, distributed sync lock, automatic migration or
age-based collection.

The additive public plugin parameters are:

```json
{
  "bootstrap_destination": "metadata",
  "analysis_artifacts": {
    "metadata_directory": "/absolute/output/.basecraft/analyses-v1/runs/RUN-ID",
    "working_directory": "/absolute/output",
    "run_id": "RUN-ID"
  }
}
```

The host allocates once against the actual final primary output. It injects
existing absolute directories and rejects caller-supplied allocation or explicit
`bootstrap_trees` ambiguity. `bootstrap_destination` accepts `metadata` (default)
or `working`. The distance wrapper selects `bootstrap-RUN-ID.nex`; the native
engine owns streaming, temporary files and atomic no-clobber final/partial publication.
The host never republishes/copies that collection. Successful output contracts or
validated structured recovery reports/diagnostics authorize registration. Existence
alone never authorizes adoption. A complete collection can survive a failed cache,
primary or report publication; the job remains failed and collection status/count
are recorded separately. Direct plugin callers can use an explicit absolute
`bootstrap_trees` path instead of allocation; the native engine need not know ops
or hidden-store layout.

`input.json` records schema version 1, submitted BLAKE3 hash/size, transformation,
source reference and evidence state. Clean disk sources reuse the hash established
by the verified document open plus size and timestamp; device/inode support local
alias detection, not portable identity. References are relative to their run where
possible. Matching stat metadata is a change signal, never proof of identical bytes.
No whole-input identity hash is recomputed on save-warning/UI queries. Explicit
verification, preservation and relinking check content hashes.

`canonical-fasta-v1` means the ops alignment serializer (headers/descriptions and
60-column residue lines, padding from the loaded alignment). `ungapped-fasta-v1`
means the ops ungapped serializer (row names, gap removal, 60-column lines). The
actual submitted hash is independent of the original raw-file hash. trimAl also
records its neutral-ID mapping and diagnostic log. Dirty input has no disk-source
claim: `generated` retains one exact `submitted.fasta`. A verified clean source
may be reconstructed into a disposable canonical cache on explicit tree extraction;
the cache is never substituted for missing/changed original evidence.

Evidence states are `referenced`, `generated`, `preserved` and `broken`.
Before replacement, the shared Session save guard inspects known stores and
ancestor-folder stores, using metadata only. It lists direct and declared downstream
analyses. Cancel performs no replacement. Preserve-copy verifies/copies the actual
old source into affected direct runs before any replacement and publishes evidence
state before save; descendants follow the owning ledger. If old bytes are missing
or changed, preservation is refused. Remove-analysis moves only affected hidden run
records into store trash; scientific results and user source files remain. Break-
evidence keeps records/results and explicitly marks evidence broken. Failed decisions
abort the save; partial preservation/trash decisions remain recoverable. Save As
new paths normally needs no decision and keeps references to the contributing
ledger. CLI/MCP/default Rust Session saves return a structured conflict/options
without an interactive prompt; Rust callers can choose `save_with_evidence`.

External editors cannot be intercepted. Verification reports changed/missing
originals. Verified-original relink accepts only matching bytes and updates the
read-only source location. No startup scan/hash of a filesystem occurs. Unknown
input/reference/registry fields survive updates; future schema versions fail.
`upstream` entries reference immutable publication ID/hash/location. Bounded
verification traverses declared records and their owning evidence decisions.
Older companion publications and schema-1 tree links remain readable. New inferred
schema-2 links refer to the run, not to a nonexistent `.alignment.afa` snapshot.
Existing explicit presentation/Save-As guard companions remain compatible.

Bindings select the hidden run for a primary/component basename. The run retains
`execution.json`, a compact `run.json`, optional publication/reports and declared
artifacts. Application-state job records point to the artifact run rather than
retaining duplicate clean inputs. Optional provenance None/LocalOnly remains
separate from the minimum analysis/evidence record needed for these protections.
Complete export is explicit: it materializes the dependency closure in a new
package and provides a standard tree/alignment association when applicable.

Host input-I/O counters cover post-open source-identity hashing, execution-scratch
reads and persistent generated-input copies. They exclude initial document loading,
mmap page faults and external-tool reads. Preservation records actual verified
copied bytes. They establish avoidance of extra host I/O, not storage-device timing
or biological reproducibility.
