# Native delimitation engine

Preparation supports exact aligned DNA haplotypes, explicit imported-time-tree
validation, manual time cuts, and fixed-root constrained relative least squares.
See ADR 0011 for the objective and real matched LSD2 checks.

Versioned requests and results are in basecraft-protocol and generated schemas.
Run `basecraft-delimitation --request REQUEST.json`; `--hash PATH` computes the
BLAKE3 input fingerprint. Native results include terminal failure/cancellation,
immutable input hashes, method parameters and optional SPART/metadata artifacts.
The bundled public plugin uses a retained immutable input copy.

GMYC single, multiple and tiny exhaustive searches use established pooled
plug-in rates, event intervals through the present, log-domain arithmetic, and
reference-start Nelder-Mead exponent fitting. Multiple starts are diagnostics,
not silent replacements. Multiplicities never enter the likelihood. Searches
have deterministic ties, cancellation boundaries and an explicit candidate
budget. The redesigned estimator is deferred to v2 and absent here.

**GMYC reference acceptance remains pending.** Analytic likelihood/rate checks,
time scaling and worker reproducibility are implemented and pass; these are not
executed R/splits comparisons. The inspected splits source clamps recent ages at
1e-6; native input is not silently clamped. The current source comparison is
`tomezard/splits` 1.1 at dc187fd (GMYC file unchanged from initial upload). Pinning
and executing the published 1.0-20 release remain release prerequisites. The
reference split/fuse neighborhood is implemented, but search-start and exact tie
behavior require that comparison before a normal GUI analysis action is enabled.
An exhausted multi-threshold budget returns a failed/incomplete result, never a
completed hypothesis. Convergence alone never implies statistical support.

## ASAP

`asap::analyse` is a native port of the reference ASAP command-line core
(Achaz and Brouillet). It takes a complete distance matrix in Basecraft's
triangular order; the application supplies `basecraft-distance` p, JC69 or
K2P distances. `Stream::Reference` reproduces the reference program exactly
for a fixed seed on Linux/glibc (glibc `rand()`, glibc sort tie order and libm
`log`/`exp`), single-threaded. `Stream::Parallel` keys one stream per
simulation by seed, node and replicate and runs on the rayon pool; results do
not depend on the thread count.

`tests/asap_reference.rs` compares both distance sources with reference
tables in `testdata/fixtures/asap/`. `examples/asap_bench.rs` drives the
benchmark. Results, reference defects and timings:
`methods_research/asap_native/README.md`. Integration with the delimitation
plugin is separate.

