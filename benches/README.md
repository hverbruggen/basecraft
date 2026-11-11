# Interaction measurements

`just bench` builds and measures the verified release runner. `just bench-record`
explicitly appends portable summaries to `benches/history.jsonl`. It covers Session open/edit/undo/
save, 4K alignment and genome frames, scrolling, and 50,000-tip tree open/reroot.
`just bench --large` also opens, edits and undoes an edit in a 1 GB FASTA.

The runner holds an exclusive compile lock and refuses to time a busy host. Set
`BASECRAFT_COMPILE_LOCK` (or `--compile-lock`) to the lock shared by all builds
on that host; the default cache lock only coordinates benchmark runs. Set
`CARGO_TARGET_DIR` and `TMPDIR` outside the source checkout. Raw logs and fixtures
use `BASECRAFT_CACHE_DIR/benchmarks` (or the user's cache by default). The runner
removes only fixtures it created. `BASECRAFT_BENCH_MACHINE` is an optional neutral
comparison label; never use a hostname. No label is inferred from the machine.

`budgets.toml` defines absolute limits. `just ci` runs exact read/sync/heap/frame
budgets and three compact timing samples against twice the normal limits.
History never changes a limit. CI and
ordinary benchmark runs do not write history; only `bench-record` does.
A failed measurement or missing budget fails the command.
Archive builds record a source SHA-256 and a null commit unless the caller sets
`BASECRAFT_BENCH_REVISION` to the full archive commit.

Default file timings use warm OS caches. `--cold` also evicts each selected large
FASTA using `POSIX_FADV_DONTNEED` before every sample and requires physical read
bytes at least as large as the fixture. `--startup` measures the real application
with Qt offscreen software rendering, a fresh profile and executable cache
eviction. Shared library caches remain uncontrolled. It measures spawn to the
first-frame trace receipt and same-process idle RSS after 1 second. These do not
qualify desktop GPU rendering or a cold operating-system boot. `REPORT.md`
separates these measurements from product requirements.
