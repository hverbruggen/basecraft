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
`budgets.toml` preserves the plan's 30% benchmark and 2x gross-CI comparison
semantics as observations. No timing gate is enabled, no single run establishes
a trusted threshold, and no direct PRD target is asserted by these headless
scenarios. First-paint/input-to-screen, tree, peak memory and startup
measurements remain subsequent work. CI continues to run existing exact counters;
its timing placeholder is intentionally deferred to the later gate stage.

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
