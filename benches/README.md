# Core interaction measurements

Run on the designated build host with Python 3.11+ and the locked Rust toolchain,
under that host's **exclusive shared compilation lock for the entire command**
(build and measurements). Do not compile or benchmark concurrently. Configure
external `CARGO_TARGET_DIR` and compiler `TMPDIR`; keep the combined build cap at
24 CPUs/jobs. Site-specific remote access, locks and runtime configuration belong
in private tooling, not this repository.

```sh
just bench --machine workstation-a --cache "$BENCH_CACHE" --samples 5
```

`BENCH_CACHE` must be an explicit absolute external cache location. The runner
builds only the headless `basecraft-ops` bench in **release** profile, offline with
`Cargo.lock`, then invokes its executable directly. Compiler and process startup
time is excluded. Source must be committed and clean. An exact `git archive`
can supply `--revision COMMIT`; private validation may reuse a matching release
binary via `--executable PATH`. Its embedded source revision must match, and debug
assertions must be disabled. Reports retain the binary hash and runner hash.
Neither option proves an arbitrary externally supplied binary's compiler flags;
retain the build command/toolchain evidence alongside private measurements.

Each sample runs in a separate process and isolated application profile with
normal default recovery/durable journaling. Deterministic fixtures, one explicit
untimed warmup open (including index caches), and Session construction precede
timing. This is **warm-cache wall time**, with no cache eviction or physical disk
read claim. `Session::open` excludes GUI routing, frames and first paint. Edit and
undo sum individual call durations then divide by 1000; edit includes first-edit
recovery promotion and ordinary journaling. Operation construction, result
cleanup, verification, process startup and teardown are excluded. Summing
individual calls includes timer overhead; these are core API measurements, not
keystroke-to-pixel latency. Save times the full existing `Session::save`, including
its current safety checks. No production behavior is changed for these numbers.

Defaults: 32 x 4096 deterministic alignment; exact 100,000,000-byte single-record
FASTA; 1000 row-0 replacements with `N`, undo; save the edited alignment over its
source. All alignment rows/names/lengths and the complete large sequence payload
are checked outside timing. Save is reopened in a new Session and verified.
Fixture generation, warming and verification may themselves take substantial
time; do not mistake total runner time for a scenario measurement.

`--large` explicitly adds a 1,000,000,000-byte FASTA open. A failed/refused
operation is recorded as `failed_or_refused`, never as successful timing, and
makes the runner exit nonzero. Large files, temporary profiles and provenance
are generated only in the external cache. Successful samples remove their own
disposable fixtures/profile after verification; raw JSON/stdout and stderr stay
in the run directory. Failed samples keep their files for diagnosis.

The report records medians in ns/operation, sample counts, fixture/operation
parameters, source commit, release profile, neutral user-supplied machine label,
UTC date and runner/executable SHA256 identities. Do not use a real hostname as a
label. Raw samples and reports may contain private runtime diagnostics: review
and sanitize any material selected for publication.

`--baseline PATH` compares a reviewed compatible report on the same machine
label, profile, runner protocol/hash and fixture parameters. Missing/incompatible
baselines explicitly report `unavailable`; they are **not a passing budget**.
`budgets.toml` defines two **opt-in** strict limits: `--gate gross` fails above
2x; `--gate benchmark` fails above 1.3x. Equality passes. Without `--gate`, ratios
remain observational. CI's timing hook stays off pending reviewed repeat-run
calibration and portable baseline provisioning. Exact-count and requested-heap
checks remain enabled and unchanged. These headless limits do not qualify PRD
first-paint/input-to-screen/startup targets.

```sh
# Hold the site's exclusive compile lock through BOTH runs.
just bench --machine workstation-a --cache "$BENCH_CACHE" --compact \
  --samples 5 --environment "$BENCH_ENVIRONMENT"
# BASELINE is the reviewed report.json from the baseline run.
just bench --machine workstation-a --cache "$BENCH_CACHE" --compact \
  --samples 5 --environment "$BENCH_ENVIRONMENT" --baseline "$BASELINE" --gate gross
```

`--compact` selects open/edit/undo/save alignment only, excluding the 100 MB open.
Both gate modes require **all four** compact scenarios in both reports with at
least five verified samples each. Extra selected scenarios must also compare
successfully. A failed/refused workload, missing required scenario, insufficient
samples, unavailable comparison, invalid median or exceeded ratio fails nonzero.
The aggregate report retains gate status, threshold and failure reasons; an
unreadable baseline or invalid environment descriptor also fails nonzero. Reports
retain minimum, maximum and median absolute deviation (MAD), in ns/operation,
alongside medians and external raw samples. No outlier is automatically discarded.
Do not remove a failing workload or disable recovery to obtain a passing gate.

`--environment` names an external, private JSON object with nonempty string
fields `hardware`, `toolchain`, `runtime`, `storage`, `build`, and `isolation`.
Describe CPU/memory/power policy, exact compiler/Cargo, OS/container image and
Python, storage/filesystem, release flags/features/target, and shared-lock/load
policy respectively. Retain the descriptor plus actual tool/version/load receipts
with the raw measurements. Its canonical SHA256 is recorded as
`environment_sha256`; strict gates require matching identities. This is a reviewed
caller attestation, not automatic hardware detection or proof of isolation.
Changing either descriptor or runner requires a fresh baseline; old reports are
never relabelled. Revision and executable hashes may differ to compare production
changes, while runner/work/fixture/environment identities must match. The runner
hash already versions harness changes, so the additive summary fields do not
reinterpret historical schema-2 timings.

Before enabling a default CI timing gate, provision a reviewed baseline and its
environment descriptor outside Git for each supported execution environment,
retain repeat complete runs and variability under the shared lock, and have the
reviewer validate the calibration and provisioning/failure path. No host-specific
baseline, private environment descriptor or fabricated timing belongs in Git.
There is currently no portable baseline provisioner; the CI hook explicitly
reports off. Use the explicit gate command at an authorized benchmark checkpoint.

`--history PATH` opts into appending summary reports to an **external** JSONL
history. Tracked `history.jsonl` is deliberately not auto-written. Review neutral
labels, measurements and environment/load evidence before selecting any public
baseline. Archive reports privately with compiler/runtime identity and host/load
observations; reject obviously noisy runs and do not extrapolate to other hosts.

Run reporting checks without compiling:

```sh
python3 -m unittest discover -s benches -p 'test_*.py'
```

Report and raw-sample **schema version 2** defines `fixture_bytes` as the generated
input file size, captured before warmup or any measured operation. Raw samples
also record `output_bytes` after the scenario for diagnostics; output size is
not part of input compatibility or the timing summary identity. Writer wrapping
changes can therefore remain comparable while changed input sizes/work parameters
still reject comparison. Version 1 save samples used the overwritten output size
as `fixture_bytes`; they must not be reinterpreted. Old reports are incompatible
with version 2 and old raw samples are rejected by the summarizer. Preserve them
as historical evidence and regenerate affected baselines under version 2.

Use `--scenario save-alignment` to refresh just the affected scenario; the option
is repeatable and defaults to the full compact/100 MB set when omitted. `--large`
continues to explicitly add the opt-in 1 GB case.

Frame scenarios are opt-in through the same runner, raw/report schema 2 and
failure handling. Existing default Session scenarios and `session-warm-v1`
meanings are unchanged. The runner hash now also covers the frame module and
bench dependency manifest, so previous runner reports remain historical and
require new baselines rather than silent comparison across this extension.

```sh
just bench --machine workstation-a --cache "$BENCH_CACHE" --samples 5 \
  --scenario frame-first-alignment --scenario frame-warm-alignment \
  --scenario frame-scroll-alignment
```

Each of `first`, `warm`, `scroll` is available for `alignment`,
`alignment-hidden`, `genome-linear`, `genome-wrapped`, and
`genome-wrapped-hidden` (prefix every combination with `frame-`). Alignment
fixtures are 256 or 1024 rows x 4096 bases, repeated ACGT with rowNNNNN names.
Genome fixtures have 100,000 bases (linear/wrapped), or 1,000,000 bases (wrapped
hidden-data scaling), repeated ACGT and two `gene=marker` annotations per 100 bp
on opposite strands. Larger cases increase hidden rows or wrapped layout lines
at the same 3840 x 2160 viewport and 8 px/base. They deliberately test CPU work
that instance counts cannot bound. They are synthetic, not biological datasets.

Fixtures are generated to actual JSON input files in the sample cache, decoded,
checked, and used to construct real documents and interval indexes **before**
timing. `fixture_bytes` still means generated input file bytes; neither frame
buffer bytes nor output instance counts replace it. There is no timed loader.
The dev-only ops -> viewmodel dependency confines this public headless client to
tests/benches; production dependencies and layer direction remain unchanged.

`first` times fresh view construction (including alignment sync), empty Frame
construction and the first real build, including initial wrapped-layout caches.
It means a cold **view**, not cold OS/data caches. No frame build precedes this
timer in that process. `warm` performs one untimed origin build, then times one
origin build with reused view/cache/Frame capacity. `scroll` warms once, then sums
200 separately timed scroll-navigation + build calls at fractions i/199 from
origin through the clamped final page (both alignment axes; the genome scrollbar
API chooses linear/wrapped behavior). The mean across those 200 calls is one
sample; the report median is across process samples, not a frame percentile.
Untimed verification between builds perturbs caches; these are isolated CPU
work measurements rather than an uninterrupted interactive animation trace.

Frames are consumed through `black_box`. Outside timers every frame must have
finite coordinates, at least 400 labelled glyphs and 400 residue-size backgrounds;
genome annotations must fit. All float payloads must equal an untimed fresh-view
replay at the same position. Raw samples retain every output instance count;
counts are diagnostic outputs, not input compatibility parameters. `work` holds
phase, frame count, dimensions, zoom, theme, fixture/annotation parameters and
scroll path and is mandatory in frame sample/comparison identity. Failures never
become timings. Existing exact-output budget tests remain separate strict checks.

These headless builders measure no pixels, Qt/GPU rendering, GUI first paint,
input-to-screen latency or allocation/peak memory. No PRD qualification or new
timing gate follows from these observations.


Tree cases are opt-in in the same executable/report:

```sh
just bench --machine workstation-a --cache "$BENCH_CACHE" --samples 5 \
  --scenario tree-open-50000 --scenario tree-midpoint-50000
```

Both generate the same 50,000 unique `tipNNNNN` names in a balanced binary
Newick tree: recursively split each contiguous tip range into floor(count/2)
and the remainder (maximum depth 16). Every nonroot branch has the positive
integer length `1 + (start*17 + count*13) % 97`, where start/count describe that
subtree. Rooting is explicitly unrooted. Generation, source hashing, one untimed
recoverable warmup open, and construction of the measured Session precede timing.
`tree-open-50000` measures only `Session::open`. `tree-midpoint-50000` measures
one typed `Op::TreeMidpointRoot` through `Session::apply`, including its normal
candidate preparation, whole-tree undo state, first-edit recovery promotion and
durable journaling. It is a first edit, with no prior edit or recovery promotion.
Undo/redo/replay are untimed verification, not a separate undo timing scenario.
No scientific implementation or production fast path is changed.

Outside timing, bounded traversals verify every tip identity exactly once,
reachability, reciprocal parents/children, binary topology (allowing a degree-three root at a midpoint endpoint) and finite
nonnegative branch lengths. The rerooted tree must preserve total branch length
within relative 1e-12; raw `output_rooted` retains the actual exchange declaration
(the current operation preserves the source declaration). Undo must restore original Newick/rooting semantics
and Session save hashes; redo must reproduce the rerooted semantics. The normal
recovery journal must contain the operation and pass the existing replay verifier.
Source bytes/hash remain unchanged. These are structural/round-trip checks, not
an independent all-pairs scientific proof or an assertion about GUI performance.

Mandatory tree `work` metadata includes tip count, shape, name/branch generator,
method, recovery mode, operation count and exact input hash. These participate
in sample/baseline compatibility using additive schema-2 metadata. The runner
hash now covers the tree module; previous reports remain historical and are
incompatible by runner hash, without relabelling old samples. Default Session and
frame scenario definitions, input/output byte meanings and tracked history stay
unchanged. Refused/failed tree operations use the shared explicit nonzero failure
path and retain diagnostics/fixtures; they never become successful timings.

Requested-heap scenarios use the same runner in a **separate invocation**:

```sh
just bench --machine workstation-a --cache "$BENCH_CACHE" --samples 5 \
  --scenario heap-open-compact --scenario heap-open-100mb --scenario heap-open-error
```

The runner builds with the explicit `test-heap` feature only for these scenarios.
Only the benchmark and dedicated integration-test binaries opt into the counting
System allocator. Normal application builds do not enable it. Instrumented
binaries refuse timing scenarios, and mixed heap/timing requests are rejected;
these runs are not comparable to uninstrumented wall timings.

`system-requested-live-v1` counts requested sizes of successful Rust global
allocations on **all threads**, from process start. Deallocation subtracts the
original requested layout; successful realloc replaces old size with new size,
including shrinking, while failed realloc retains the original allocation.
Hooks serialize System calls and accounting with an allocation-free spin lock.
They do no logging, TLS initialization, collection allocation or formatting.
Overflow/underflow invalidates subsequent measurements rather than wrapping.
Only one process-wide measurement owner is permitted; drop releases it on error.
Unrelated threads would contaminate a measurement, hence isolated sample processes.

The window covers exactly the real recoverable `Session::open` call, including
its normal loading/recovery work and error cleanup before returning. Generation,
profile and Session construction, warmup and full bounded-buffer payload checking
are outside the window. Compact uses an exact 4,096-byte single-record FASTA;
large uses the existing exact 100,000,000-byte generator. Error uses malformed
FASTA through the real loader, without a warmup that could consume its failure.
The opened Session is dropped after verification, before the cleanup reading.
No production loader/recovery code is changed.

Raw `heap` fields report existing `baseline_bytes`, total `peak_bytes`,
`incremental_peak_bytes = peak - baseline`, retained `live_after_open_bytes`,
and `live_after_drop_bytes` (absolute process live requested bytes), plus successful
realloc and allocation-failure counts. Peak starts at the baseline even if live
bytes subsequently fall. Summary medians retain all these fields in
`heap_medians`; the primary `median`/`unit` is incremental peak **requested bytes**,
not ns/operation. Mandatory `work` includes instrumentation identity, units,
window, recovery, warmup, fixture and expected-error semantics. Runner hashing
covers shared fixtures and allocator implementation. Historical reports remain
historical; missing/different instrumentation or units cannot be compared.

This does **not** measure allocator metadata, size-class slack, transient internal
copies during System realloc, memory mappings, foreign allocations bypassing
Rust's global allocator, process RSS, GUI/startup memory or the 1 GB boundary.
Neither total nor incremental requested bytes are total process memory.

The deterministic `heap_budgets` integration test joins `just check`/`just ci`.
It generates its own compact and 100 MB inputs in the configured temporary
storage, never skips a missing external fixture, and checks the real error path.
Incremental peak must be at most **2 × input file bytes + 65,536 requested bytes**;
after Session drop absolute live requested bytes must be at most the pre-open
baseline + 65,536. Total peak/retained values are also printed. These are gross
current-behavior regression bounds, not PRD qualification. Benchmark reports
remain observational; the deterministic Rust test is the enabled memory gate.

Before choosing the bounds, revision `39dae81` measured five independent release
samples per scenario and the debug test. Release medians: 10,802 incremental
requested bytes for compact FASTA, 100,985,701 for the exact 100 MB input, and 355
for malformed FASTA. Debug large incremental peak was 100,985,458 bytes. The
large gate allows almost 100% headroom (about 2.0007 × file size against the
measured 1.0099 ×); the 64 KiB floor absorbs small-fixture/framework/path variation
without weakening the large bound materially. Requested-byte accounting avoids
wall-time/OS allocator noise, and the measured debug/release difference was tiny
relative to the headroom. The after-drop bound catches retained large buffers;
it permits small process-library/cache state rather than requiring exact zero.
Private raw evidence and environment identity belong with the task review, not
in tracked history. This baseline is one deterministic wrapped single-record
FASTA workload, not a universal memory scaling or 1 GB promise.

### Store inspections per save

The existing `cargo test -p basecraft-ops --features test-counters --test budgets`
command in `just check` (and therefore `just ci`) also gates production analysis
store scans. `basecraft_io::counters::store_inspections` counts **attempted visits**
at `input_evidence::scan_store` entry, before validation, including missing/corrupt
stores and each repeat. It is a map from the exact supplied path to visit count;
it performs no canonicalization or filesystem access. Aliased paths remain
separate keys. Sum values for total attempts; sum `(visits - 1)` for repeated
visits of the same supplied path. Registry reads, ancestor discovery probes,
individual run/dependency reads and non-scanner store validation are not store
visits. This is a work count, not a timing or total filesystem-I/O metric.

The counters share the existing thread-local `reset`/`snapshot` scope with read,
byte and sync counts. Reset immediately before the operation to measure that
operation; without reset, advisory checks and authoritative rechecks accumulate.
Spawned threads have independent counters. Isolated child profiles keep the real
save scanner's discovered registry self-contained. No feature means no counter
module or scanner call, and no new wire API.

Baseline captured at `7c36aef` on the otherwise unchanged scanner, before adding
exact assertions:

| Scenario | Attempts | Repeats |
|---|---:|---:|
| Default complete FASTA/GenBank/Newick save, no stores | 0 | 0 |
| Complete save with 1 / 2 / 3 unrelated registered stores | 1 / 2 / 3 | 0 |
| Clean no-op save after those saves | 0 | 0 |
| Incomplete save: one missing and one corrupt registered store | 2 | 0 |
| Bind the returned decision without resetting | 2 cumulative | 0 |
| Warned save with that decision, still without resetting | 4 cumulative | 2 |
| Advisory recheck / explicit decision save of clean document | 2 each | 0 |
| Known-input advisory, one nearby and registered store | 1 | 0 |
| Stale inventory decision after adding unrelated store | 2 | 0 |
| Fresh known-input preserve-copy save | 2 | 0 |
| Explicit `check_in` with the same missing path twice | 2 | 1 |
| No-overwrite refusal before scanner entry | 0 | 0 |

The tests compare the **entire per-store map**, so an additional scan or an
unexpected store fails the gate. The existing exact read/byte/sync budgets remain
unchanged. Global-scan cost currently grows with unrelated registered stores;
this records that known architectural gap openly and does not qualify the save
path against the desired file-local cost. Scanner/index/lock improvements and
changes to evidence authority are separate work. These checks make no GUI,
startup, elapsed-time or release qualification claim.


### Explicit opt-in CI provisioning

Default `ci` timing remains **off** until independent provisioning review and
supported-environment calibration acceptance. `bench-ci` is an opt-in invocation
of the same `--compact --samples 5 --gate gross` runner, not a `ci` dependency.
The private site wrapper must select immutable external files and hold its shared
compile lock through this command; it also selects external target/TMP storage.
No acquisition/download, baseline creation, missing-input skip or current-run
self-baseline is performed by the recipe.

```sh
just bench-ci "$BENCH_MACHINE" "$BENCH_CACHE" "$BASELINE" "$BENCH_ENVIRONMENT" \
  "$RUNTIME_RECEIPT" "$PROVISION_RECEIPT" "$PROVISION_SHA256"
```

Every argument is required. Paths identify files outside the checkout. The receipt
checksum must be pinned independently in reviewed private site configuration;
reading a checksum from the same untrusted receipt is not a trust anchor.
All examples below describe **synthetic field shapes**, not approved calibration.
No private baseline, receipt, actual host identity or artifact location belongs
in Git.

The provisioning receipt is a JSON object with these required fields:

| Field | Meaning |
|---|---|
| `schema_version` | Integer `1` (benchmark provisioning contract, not an application record) |
| `reviewed_by`, `review_reference` | Nonempty neutral reviewer and private review identifiers |
| `valid_until_utc` | Explicit timezone-bearing ISO timestamp; expired receipts fail |
| `baseline_sha256` | SHA256 of immutable baseline report file bytes |
| `baseline_source_commit` | Baseline's exact 40-digit source revision |
| `baseline_executable_sha256` | Baseline's recorded release executable SHA256 |
| `runner_sha256` | Exact runner identity from the selected baseline, matching current runner |
| `environment_sha256` | Canonical descriptor SHA256, matching baseline and current descriptor |
| `runtime_sha256` | SHA256 of the supplied current runtime receipt file bytes |

SHA256 strings are lowercase 64-digit hex. The runtime receipt is a JSON object
with nonempty strings `target` (actual compilation target), `flags` (effective
features/compiler/linker flags, explicitly stating defaults/none where relevant),
`storage` (effective storage identity/filesystem), `power_policy` (effective power
mode/governor/limits), and `load_policy` (shared lock, CPU budget and contention
policy). `image_sha256` and `toolchain_sha256` pin the immutable runtime image
and toolchain identities. The environment descriptor retains its six existing
required strings and must additionally contain `reviewed_runtime` equal to this
entire runtime receipt object. Include additional tool-version or policy evidence
there when needed; any descriptor change invalidates the baseline identity.

Before Cargo or a benchmark starts, provisioning checks checksums, receipt
expiry/schema, exact baseline source/executable pins, report schema/release/protocol,
neutral machine label, runner/environment identities, and all four measured compact
cases. Each needs at least five samples, positive finite median/minimum/maximum,
nonnegative finite MAD, consistent range, exact fixture/work counts and release
assertions disabled. Missing/unreadable/malformed/incompatible inputs fail nonzero
with a diagnostic. Report and runtime file bytes are read once for checksum and
validation. Child failures still retain stdout/stderr, exit code and aggregate
failure while subsequent cases run. Reports record provisioning/runtime checksums.
The optional direct `--gate benchmark` path keeps the existing 1.3x semantics;
`bench-ci` deliberately selects the existing 2x gross gate.

This verifies consistency with externally pinned, reviewed assertions. It cannot
authenticate an arbitrary report or reviewer, prove a binary's compiler flags,
measure actual governor/load continuously, or establish that an attested runtime
matches the machine. The site wrapper/reviewer must collect and check actual
build/runtime receipts. Expiry alone does not establish calibration freshness;
the site review chooses a validity window and must revoke the pinned receipt
when its environment/policy changes. No signatures or automatic host attestation
are claimed.

The changed runner hash invalidates all earlier runner reports. Existing samples,
including prior calibration rounds, must stay unmodified: do not relabel them or
add new runtime assertions after measurement. A new supported-environment baseline
and independent acceptance are needed before enabling default timing CI. This
Python-only provisioning change supplies no new timing/calibration measurements.
