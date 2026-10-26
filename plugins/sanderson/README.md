# Sanderson penalized likelihood (r8s 1.81)

This schema-2 file operation accepts one supplied, explicitly rooted, binary
Newick substitution tree. It fits the original-author **penalized-likelihood**
method with r8s 1.81's truncated-Newton optimizer. It does not root a tree,
choose fossils, implement NPRS, or infer a biological time scale.

The caller must give `branch_units`, `sites`, `root_age`, `time_unit`, and either
`lambda` or `cross_validation`. `substitutions_per_site` lengths are multiplied
by `sites`; `substitution_counts` lengths already are totals, while `sites`
still determines reported rates per site. r8s 1.81 **forcibly rounds the total
counts inside PL even if `round=no` was requested**. This adapter rejects every
branch that does not scale to a positive integer count. It never modifies an
original tree or silently rounds, collapses, or jitters its branches.

`root_age` fixes the root, with all extant tips at age zero. A dimensionless
choice such as `root_age: 1, time_unit: "relative units"` yields relative
times. Constraints are optional hard fixed/bounded ages on internal nodes.
`node` can be a unique internal Newick label or an `N000001` style preorder ID
(root is `N000001`; internal nodes are numbered in input traversal order).
The adapter checks ancestral feasibility and validates the returned bounds.
It supports no dated tips, uncertain fossils, polytomies, stem lengths, or
zero/missing/nonfinite/negative branch lengths.

`penalty` selects r8s additive or log-rate smoothing. `lambda` is the r8s
`smoothing` value for that variant. The optional grid uses
`10^(log10_start + i*log10_step)` for `count` points. r8s cross-validation
prunes each terminal branch, refits, and predicts its substitutions. The
adapter uses r8s's optimum selected from its unrounded chi-square-like errors
only when every grid point succeeds, then performs a fresh full-data PL fit.
The printed scores have two decimal places; grids whose printed optimum is
ambiguous are rejected. This differs from
ape `chronopl(CV=TRUE)`'s pruned-node-age criterion. Explicit `lambda` is
recorded as requested but the selected CV value controls the fit when both
are supplied.

The result contains a chronogram with the original labels and a JSON report
with original input bytes as text, SHA-256, exact supplied branch lengths,
node ages, fitted rates, settings, engine identity, CV scores, diagnostics and
raw r8s output. The shared registry owns input freezing, output allocation,
validation, publication and caller parity. r8s prints durations to six
decimal places and fitted rates to five significant figures; these reported
values do not support exact full-precision objective reevaluation. A
converged local estimate is not a confidence interval or biological validity
claim.

## Engine packaging gate

The plugin requires `bin/r8s` and `bin/engine.json` at runtime. The identity
file must state the SHA-256 of the exact packaged binary, r8s version 1.81,
and source archive SHA-256
`9e89d7851d74d74487d147b77177a717e6c659b485c9b67f516340a6ed595080`.
An analysis never downloads, installs or compiles an engine. The engine is
currently absent while the coordinator's compilation/package hold applies;
invocation returns an explicit availability error.

Author source: https://sourceforge.net/projects/r8s/files/r8s1.81.tar.gz/download.
The archive's `LICENSE` is GPL-3.0; its README says Sanderson's C is GPL and
Stephen Nash's included truncated-Newton Fortran is public domain. A released
bundle must include the applicable GPL notice and corresponding source. This
plugin's Python source is part of GPL Basecraft; the separate protocol and
SDK remain Apache-2.0.
