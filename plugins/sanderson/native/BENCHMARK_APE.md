# Native PL and ape::chronos comparison

The benchmark-only comparator is **ape 5.8-1**, installed from the pinned
[CRAN source archive](https://cran.r-project.org/src/contrib/ape_5.8-1.tar.gz)
(SHA-256 `d6cde3dda751597ad741f634f143a3eb521b957fd1a23ceefba78a2716422d8e`)
in R 4.4.2. R and ape are not plugin dependencies or runtime alternatives.
`tools/benchmark_chronos.R` uses `chronos(model="correlated")` and
`tools/benchmark_ape.py` generates shared deterministic rooted trees with
known node ages and branch rates, runs the native executable, and independently
scores both fitted trees. `tools/benchmark_ab.py` measures native build changes
in alternating order and requires byte-identical results.

## Model mapping and limits

| Setting | Native method | ape::chronos 5.8-1 | Interpretation |
|---|---|---|---|
| Input | Inferred substitutions/site, `sites=1` | Same Newick lengths | Both evaluate the same continuous `x=b`; ape calls `lfactorial(b)`, the gamma extension used natively. Ordinary product analyses can use an explicit site count instead. |
| Tree/time | Same rooted binary topology; root age 1; tips age 0 | Root calibration 1; extant tips 0 | Same relative time unit. No rooting or fossil choice is inferred. |
| Hard bound | First half internal clade fixed at true age 0.5 in the constrained case | Same clade via `getMRCA`; `age.min=age.max=0.5` | Independent scorer checks the achieved age and every duration. |
| Penalty | `add`, adjacent squared total-rate differences plus **positive population** variance of the two basal rates | `correlated`, adjacent squared rate differences; source adds **positive sample** basal variance to penalized log likelihood | Nonbasal term and rate scale match. Basal terms differ in sign and denominator, so the full objectives and equal numeric lambda are **not equivalent**. |
| Lambda | Explicit fixed `lambda=1` | Explicit fixed `lambda=1` | Same coefficient only for the shared nonbasal additive term; no claim of full-model parameter equivalence. |
| Optimizer | Argmin 0.11 L-BFGS, three deterministic starts, analytic gradient, explicit unscaled-gradient acceptance and iteration/time budgets | `nlminb` joint then alternating rates/dates, ape's supplied gradient and dual-iteration controls | Stopping rules and search paths differ. A reported ape convergence flag is retained with any warnings. |
| CV | Native terminal-branch prediction | Not run | ape's CV conventions are distinct and are excluded from this fixed-lambda comparison. |

In pinned `ape/R/chronos.R`, `penal.loglik` adds `lambda*var(rate[basal])`,
while its correlated gradient subtracts a basal-variance derivative. The
native objective and gradient both penalize the positive population variance.
The independent scorer reports likelihood, nonbasal roughness, both basal
variance conventions, the objective each source actually evaluates,
constraint feasibility, convergence diagnostics, and truth age/rate RMSE.
It re-evaluates each reported objective; a smaller cross-method score is
reported only under the explicitly named scoring convention.

ape timing wraps only the `chronos()` call inside an already running R process;
native timing wraps each whole subprocess and its JSON transport. Docker and
R startup are excluded from ape time, making small-tree elapsed comparisons
conservative for native Rust. The ape container is limited to one CPU and
BLAS/OpenMP threads to one; the native solver is serial. All timed runs occur
serially on the build host under the shared compile lock. The scripts record raw
times, binary/request hashes, and individual result diagnostics in task-owned
storage outside synced source. The SCI-0011 worker report contains exact
environment, commands, receipts, results and scientific qualifications.

To regenerate fixtures in a new external directory, run `benchmark_ape.py
prepare --out <new-directory> --tips <8|64|128|512> --kind <clock|nonclock>
--lambda 1 --starts 3` and add `--constraint` for the fixed-age case. Run
`benchmark_chronos.R <fixture> <new-output> <repetitions>` with pinned ape
installed and `APE_DUAL_ITER_MAX=20` (or explicitly 100 for the 512-tip
sensitivity run). Run `benchmark_ape.py native-run` with the same fixture and
validated binary, then `benchmark_ape.py score` with both outputs. The
benchmark-only R/ape environment must be kept outside the shipped plugin.
