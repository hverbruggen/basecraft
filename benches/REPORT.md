# Interaction budget status — 11 November 2026

These measurements are release builds of the headless core. They do not qualify
end-to-end desktop performance. The table uses five warm-cache samples per
scenario at commit `2a05000`, with the shared host compile lock held exclusively.
All 23 headless gates pass. `history.jsonl` records the exact Rust source digest
and keeps the earlier three-sample shared-host results labelled `reference`.

| Requirement or proxy | Result | Status |
| --- | --- | --- |
| Cold application start <300 ms | No target-machine cold launch measured | Unmeasured |
| Idle application RSS <100 MB | Measurement wired into build receipt; no target receipt produced in this lane | Unmeasured |
| Input to screen <16 ms | Edit 2.053 ms + first alignment frame 2.665 ms = 4.718 ms core proxy | Proxy met; end-to-end unmeasured |
| 1 GB open to first paint <1 s | No display presentation measured | Unmeasured |
| Constant-memory indexed browsing | Editable documents retain an owned input buffer | Not met by editable-document path; separate read-only design |
| 100 MB core open | 49.057 ms | Headless gate met |
| 1 GB core open | 484.610 ms | Headless gate met; first paint unmeasured |
| Compact core open / undo / save | 0.211 / 1.983 / 38.113 ms | Headless gates met |
| 4K first frame, alignment / hidden alignment | 2.665 / 2.936 ms | Headless 16 ms gate met |
| 4K first frame, linear / wrapped / hidden wrapped genome | 0.053 / 0.570 / 2.026 ms | Headless 16 ms gate met |
| 4K scrolling, worst measured case | 0.648 ms/frame | Headless 16 ms gate met |
| 50,000-tip tree open / midpoint reroot | 9.890 / 26.879 ms | Headless gates met |

`just check` includes exact read, sync, requested-heap and frame-size assertions.
Main files and format companions are read once. Opening writes no source, state,
or cache files and forces no flushes. The first durable edit reads no source
bytes and performs one journal sync. It retains a same-filesystem hard link in
application state when possible; otherwise recovery references the original.
Later edits check stamps and checkpoint the live document if the source changed.
Recovery starts from the latest completed save or checkpoint. This policy is the
same for every file size. Files must fit in RAM.

Multi-record GenBank files and multi-record FASTA files with a GFF3 companion
cannot be saved yet: their current writers would lose other records or their
annotations. Save and Save As return a clear refusal before modifying any file.

`just bench` runs the usable default scenarios; `--large` adds the 1 GB workload
and verifies an edit and its undo outside the open timer. `just ci` runs a compact
gross timing gate. `budgets.toml` contains absolute thresholds and relative gates;
these generous regression thresholds do not replace the PRD's UI requirements.
Configure the host's shared compile lock for isolated timing measurements.

`--startup-trace` records Rust main entry, QML loading, and the first swapped
frame. `tools/test_build.py` collects five launches and same-process idle RSS per
rendering backend, medians and previous-build ratios. The trace excludes dynamic
loader time, and uncontrolled filesystem caches do not qualify as a cold start.
Offscreen test-window validation is distinct from desktop startup qualification.

The [draft source-storage ADR](ADR-owned-file-sources.md) recommends owned editable
buffers and a separate investigation of lazy indexed read-only browsing. It does
not change the product's first-paint target.

## Open memory and disk reads

The compact per-kind check runs with both read counters and requested-heap
instrumentation in isolated debug-build processes. Every input below is read exactly once,
including companions. Session construction and fixture setup are outside the
measurement window; decoded document objects and counters are included.

| Kind / format | Input files | Total bytes read | Peak extra requested heap |
| --- | ---: | ---: | ---: |
| Sequence FASTA + GFF3 | 2 | 644 | 7,035 B |
| Sequence GenBank | 1 | 940 | 5,545 B |
| Alignment FASTA | 1 | 1,600 | 9,481 B |
| Tree Newick + sidecar | 2 | 649 | 5,887 B |
| Delimitation SPART + sidecar | 2 | 5,069 | 20,791 B |
| Metadata manifest + four tables | 5 | 825 | 8,995 B |
| Submission JSON | 1 | 505 | 6,364 B |

All seven cases also assert zero writes and zero forced flushes, including an
open-only simulated crash. GenBank ignores an unused same-name GFF companion.

| Large input | Bytes read once | Peak extra requested heap | Open writes |
| --- | ---: | ---: | ---: |
| FASTA, 100 million residues | 101,666,678 | 101,672,185 B | 0 |
| Alignment, 100 million residues | 101,666,690 | 101,672,611 B | 0 |
| GenBank, 100 million residues | 118,333,408 | 100,066,555 B | 0 |

FASTA and alignment meet the 1.05× file-size bound. GenBank streams into its
parsed record and stays within the record plus 8 MiB bound. These measure
requested heap, not RSS; Session setup and fixture generation are excluded.

Before cache deletion, five warm-cache release samples measured FASTA index
rebuilding at 22.011 ms for 100 MB and 209.279 ms for 1 GB. Read-and-hash medians
were 28.593 and 288.476 ms: rebuilding was 77% and 73% of that cost, respectively.
It was not a small fraction on this server. Removing the cache eliminates
open-time writes and metadata-keyed cached indexes; it retains this CPU cost.
Save peak memory and streaming publication are measured by the save lane.
