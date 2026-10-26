# Sanderson penalized likelihood

This schema-2 file operation fits a supplied, rooted binary substitution tree
with a native Rust penalized-likelihood (PL) engine. It neither roots the tree
nor chooses fossils. Its scientific calculation does not run R, r8s or Python
optimizers. The Python handler only transports the frozen input and results to
and from the native executable and host-owned artifact paths.

## Scientific convention

The caller supplies `branch_units`, `sites`, `root_age`, `time_unit`, and a
positive `lambda` or `cross_validation` grid. For per-site lengths `b`, the
continuous observation is `x = b * sites`; total-substitution lengths already
supply `x`. **No rounding is applied.** For a branch duration `d` and fitted
total-substitution rate `q`, the expected observation is `q*d`. The objective
minimizes `sum(q*d - x*log(q*d) + lgamma(x+1)) + lambda*roughness`. The gamma
term extends the Poisson factorial to continuous inferred lengths. This is a
quasi-likelihood convention; inferred lengths are not literal observed counts.
Reported rates divide `q` by `sites`.

The roughness is the sum of squared differences between each branch rate and
its parent branch rate, plus the **population variance** among root-child
rates. `penalty=add` uses total-substitution rates; `penalty=log` applies the
same structure to log rates. Lambda is variant-specific. Additive lambda also
depends on the supplied count/time scales. The root is fixed to `root_age`,
all extant tips are fixed to zero, and supplied hard internal ages must be
strictly feasible with ancestry. A dimensionless root age of 1 with
`time_unit="relative units"` yields relative times, not calendar dates.
The numerical age map reserves a duration of at least `64*f64::EPSILON` times
the supplied root age on every edge; tighter calibrations are rejected.

The optional CV grid is `10^(log10_start + i*log10_step)`. Each terminal
branch observation is withheld, its rate parameter is removed, and the other
rates and ages are refitted. The held-out count is predicted from its fitted
parent branch rate, or the sister root branch rate for a basal tip, times its
estimated duration. The engine records squared and expected-count-normalized
squared prediction errors, chooses the minimum latter only if all folds
converge, then fits the full tree at that lambda. This reproduces the
branch-prediction structure inspected in r8s 1.81 source, with the continuous
observation convention above. It differs from ape `chronopl(CV=TRUE)`'s
pruned-node-age criterion. Full CV is limited to 128 tips pending measured
scaling and budget review.

The engine uses indexed tree arrays and analytic gradients, optimized with
argmin 0.11 L-BFGS and a More-Thuente line search. Repeated starts are
seeded deterministically; work arrays and the last objective/gradient are
reused. Every fit has explicit iteration/evaluation/time budgets and checks
host-owned cancellation. Native age/rate/constraint/convergence checks run
before the host publishes outputs. A local optimum is not a confidence
interval or evidence that topology, calibration or biological history is
correct. Dated tips, polytomies, soft bounds, zero/missing/nonfinite lengths
and automatic biological policy are unsupported.

## Source and packaging state

`native/` is an isolated Rust crate while WF-0017/coordinator owns the root
Cargo workspace and lockfile. A built `bin/basecraft-sanderson-native` must be
bundled atomically with the plugin; no analysis installs or compiles it.
The current build hold prevents compilation, numerical execution, benchmarks,
license closure for resolved dependencies, and packaging. The plugin therefore
fails explicitly until the native binary is validated and packaged. The
private worker report tracks these separate gates.

Scientific references: Sanderson (2002), DOI
[10.1093/oxfordjournals.molbev.a003974](https://doi.org/10.1093/oxfordjournals.molbev.a003974),
and the inspected original-author r8s 1.81 `TimeAlgorithms.c` penalty/CV source.
The full original paper was not available in the retained SCI-0010 research
material, so literal paper-equation parity has not been established.
Argmin 0.11 is dual MIT/Apache-2.0 licensed; dependency license notices and
exact package identity require closure with the compiled lockfile.
