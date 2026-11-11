# Interaction measurements

`just bench` builds the verified release runner and appends one portable JSONL
summary per scenario to `benches/history.jsonl`. It covers Session open/edit/undo/
save, 4K alignment and genome frames, scrolling, and 50,000-tip tree open/reroot.
`just bench --large` also opens, edits and undoes an edit in a 1 GB FASTA.

The runner holds an exclusive compile lock and refuses to time a busy host. Set
`BASECRAFT_COMPILE_LOCK` (or `--compile-lock`) to the lock shared by all builds
on that host; the default cache lock only coordinates benchmark runs. Set
`CARGO_TARGET_DIR` and `TMPDIR` outside the source checkout. Raw logs and fixtures
use `BASECRAFT_CACHE_DIR/benchmarks` (or the user's cache by default). The runner
removes only fixtures it created. `BASECRAFT_BENCH_MACHINE` is an optional neutral
comparison label; never use a hostname. No label is inferred from the machine.

`budgets.toml` defines absolute headless limits and relative regression ratios.
`just ci` runs exact read/sync/heap/frame budgets and one compact timing sample
with a generous gate. Named-machine runs compare against the last passing
compatible history line (2x in CI, 1.3x in benchmarks); unlabelled runs use the
absolute limits. A failed measurement or missing budget fails the command.
Archive builds record a source SHA-256 and a null commit unless the caller sets
`BASECRAFT_BENCH_REVISION` to the full archive commit.

Timing samples include warm OS caches and are sensitive to shared-host load.
They are not cold-start or first-paint measurements. `REPORT.md` separates the
measured headless proxies from the product requirements. Startup tracing is
opt-in (`--startup-trace`); the build delivery tool records five launches and idle
RSS for each rendering backend in its receipt.
