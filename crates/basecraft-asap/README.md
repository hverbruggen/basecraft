# basecraft-asap

`basecraft_asap::analyse` is a native port of the reference ASAP command-line
core (Puillandre, Brouillet and Achaz 2021; code by G. Achaz and S. Brouillet).
It takes a complete distance matrix in Basecraft's triangular order; the
application supplies `basecraft-distance` p, JC69 or K2P distances.
`Stream::Reference` reproduces the reference program exactly for a fixed seed
on Linux/glibc (glibc `rand()`, glibc sort tie order and libm `log`/`exp`),
single-threaded. `Stream::Parallel` keys one stream per simulation by seed,
node and replicate and runs on the rayon pool; results do not depend on the
thread count.

`tests/reference.rs` compares both distance sources with reference tables in
`testdata/fixtures/asap/`. `examples/asap_bench.rs` drives the benchmark.
Results, reference defects and timings: `methods_research/asap_native/README.md`.
`basecraft-delimitation` runs it for the `asap` analysis.
