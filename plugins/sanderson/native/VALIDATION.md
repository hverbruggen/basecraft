# SCI-0011 standalone validation recipe

The user lifted the hold for this standalone crate; broader Basecraft
integration and package builds remain held. All compilation and Rust tests run
on build-host under the shared compile lock, with at most 24 combined cores,
an isolated build profile, and both
`CARGO_TARGET_DIR` and `TMPDIR` outside the synced checkout. Coordinate the
standalone `native/Cargo.toml` and its resolved lockfile with WF-0017 before
connecting it to the root workspace or publication recipe. This file records
commands for reproducing the standalone method checks. The SCI-0011 private
worker report records the executed commands and receipts.

1. Freeze the exact SCI-0011 commit and toolchain. Resolve/check the pinned
   `argmin=0.11.0`, `argmin-math=0.5.1`, `statrs=0.18.0` dependency tree and
   licenses. Build and test the standalone manifest on build-host with the
   shared lock and external target/temp storage:

   ```sh
   cargo test --locked --manifest-path plugins/sanderson/native/Cargo.toml
   cargo clippy --locked --manifest-path plugins/sanderson/native/Cargo.toml --all-targets -- -D warnings
   cargo build --locked --manifest-path plugins/sanderson/native/Cargo.toml --release
   ```

2. Check analytic gradient against central differences for both additive and
   log-rate penalties, the independent direct objective evaluator, fractional
   per-site lengths, invalid/calibration input, held-out branch prediction,
   clock/nonclock fits, lambda sensitivity, repeated starts, bound activity,
   cancellation, evaluation/iteration/time limits and output shape. Compare
   positive integer-count cases to pinned original-author r8s outputs only as
   reference evidence; do not run r8s as the product engine. Explicitly record
   the continuous-count extension's difference from that reference.

3. Generate deterministic scientific fixtures on an unsynced volume with
   `native/tools/generate_fixture.py`: `--tips 8 --kind clock`, `--tips 64
   --kind nonclock`, and `--tips 512 --kind nonclock`. The generator writes
   `tree.nwk` plus the exact native `request.json`. Supply `--out` for a new
   directory for each fixture. A fourth small nonclock fixture can exercise
   CV by replacing `cross_validation: null` with an explicit log grid in a
   disposable copy of the request.

4. Measure the release executable end to end with
   `python3 tools/measure.py <binary> <fixture>/request.json <output.json>`.
   The helper uses Linux child-process resource accounting to report elapsed
   time and peak RSS; it does no scientific calculation. Repeat each size on
   the same machine/toolchain. For the objective benchmark, run from this
   crate directory:

   ```sh
   BASECRAFT_SANDERSON_BENCH_REQUEST=<fixture>/request.json cargo test --locked --release --lib benchmark_objective_against_direct_reference -- --ignored --nocapture
   ```

   The test verifies the independent objective and finite-difference gradient.
   It reports cost-only versus indexed cost-plus-gradient separately from the
   joint finite-difference baseline. Timing occurs inside the test, after
   compilation. Retain raw timings, peak RSS, hashes and convergence results.

5. After WF-0017 is accepted, package the exact validated native executable
   at `plugins/sanderson/bin/basecraft-sanderson-native`, include dependency
   license notices/source facts, and run the schema-2 operation via GUI,
   CLI, Python and headless MCP through the shared registry. Check denied
   execution, immutable input identity, destination collision, malformed or
   truncated native output, cancellation and atomic host publication. Do not
   compile, install dependencies or create successful artifacts at analysis
   time. Coordinator alone handles integration, push and publication.
