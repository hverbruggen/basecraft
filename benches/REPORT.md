# Interaction budget status — 11 November 2026

These measurements are release builds of the headless core. They do not qualify
end-to-end desktop performance. Initial measurements use three warm-cache
samples on a shared host, labelled `reference` in `history.jsonl`; the history
records the exact Rust source digest. Shared-host results are provisional.

| Requirement or proxy | Result | Status |
| --- | --- | --- |
| Cold application start <300 ms | No target-machine cold launch measured | Unmeasured |
| Idle application RSS <100 MB | Measurement wired into build receipt; no target receipt produced in this lane | Unmeasured |
| Input to screen <16 ms | Edit 2.210 ms + first alignment frame 2.931 ms = 5.141 ms core proxy | Proxy met; end-to-end unmeasured |
| 1 GB open to first paint <1 s | No display presentation measured | Unmeasured |
| Constant-memory indexed browsing | Editable documents retain an owned input buffer | Not met by editable-document path; separate read-only design |
| 100 MB core open | 74.745 ms | Headless gate met |
| Compact core open / undo / save | 5.860 / 2.273 / 42.334 ms | Headless gates met |
| 4K first frame, alignment / hidden alignment | 2.931 / 3.428 ms | Headless 16 ms gate met |
| 4K first frame, linear / wrapped / hidden wrapped genome | 0.061 / 0.597 / 2.833 ms | Headless 16 ms gate met |
| 4K scrolling, worst measured case | 0.689 ms/frame | Headless 16 ms gate met |
| 50,000-tip tree open / midpoint reroot | 17.571 / 23.586 ms | Headless gates met |

`just check` includes exact read, sync, requested-heap and frame-size assertions.
Main files and format companions are read once. A first durable edit reads no
source bytes and performs one journal sync; recovery staging happens on open.
Files larger than 100 MiB are editable and use verified references for recovery.
The large-file regression covers edit, Save, undo, crash recovery, and changed or
missing originals. File size must fit in RAM.

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
instrumentation in isolated processes. Every input below is read exactly once,
including companions. Session construction and fixture setup are outside the
measurement window; decoded document objects and counters are included.

| Kind / format | Input files | Total bytes read | Peak extra requested heap |
| --- | ---: | ---: | ---: |
| Sequence FASTA + GFF3 | 2 | 644 | 16,117 B |
| Sequence GenBank | 1 | 940 | 16,215 B |
| Alignment FASTA | 1 | 1,600 | 18,198 B |
| Tree Newick + sidecar | 2 | 649 | 15,438 B |
| Delimitation SPART + sidecar | 2 | 5,069 | 27,363 B |
| Metadata manifest + four tables | 5 | 825 | 19,313 B |
| Submission JSON | 1 | 505 | 14,379 B |

The separate 100 MB FASTA budget measured 100,013,801 B of incremental requested
heap, and released it when the Session was dropped. This is approximately one
input-sized buffer, not an RSS measurement. Small parsed formats have fixed and
decoded-object overhead, as the table shows. Recovery-disabled sessions share
the same immutable source allocation rather than retaining a second raw copy.
