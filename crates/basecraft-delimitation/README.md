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

Time-tree preparation fits a zero-age tree clade as one tip and restores its
members in reported assignments. This tree preparation does not establish
exact sequence identity or infer why the input tree has that clade. Separately,
multiple-threshold and exhaustive searches reject nested internal events with
exactly equal validated ages, including root-age ties and ties created by the
explicit reference age clamp. A partition can split such an event across a
zero-duration interval and produce unbounded likelihood. Single-threshold
search remains supported for these inputs. Equal ages in separate clades and
strictly positive intervals remain supported by the multi-threshold guard.

Historical reference comparisons were executed on 2026-10-12 against unmodified
`splits` 1.0-20 and 1.1 (`dc187fd`). Of 32 attempts, 28 completed comparisons
and four were recorded as upstream output-assembly failures. Selected
memberships agreed in 26/28 completed comparisons; the differences and
likelihood/profile tolerances are documented in ADR 0014. Comparisons covered
four-tip, simulated 8/16/32/64/100/128-tip, and eight-tip ladder trees, plus
separately tied and very short positive branches. They did not establish
empirical biological validity or cover large multi-threshold searches. On the
ladder, the selected null and two-entity fits differed by only 4.8793e-9 in
likelihood; both remain in the support profile. Selected-fit absolute tolerance
was 1e-5; complete-profile tolerances were 1e-3 for single and 1e-3 for
multiple fits. The maximum observed complete-profile errors were 0.000783489
and 0.00004787, respectively.

Both reference versions clamp recent ages at 1e-6 input units; native input is
not silently clamped. An explicit matching clamp is available for reproduction.
The reference split/fuse neighborhood is implemented, but it is a heuristic,
not a global optimizer. An exhausted multi-threshold budget returns a
failed/incomplete result, never a completed hypothesis. Convergence alone never
implies statistical support.

## ASAP

The `asap` method computes `basecraft-distance` p, JC69 or K2P distances from
the immutable aligned FASTA and runs `basecraft-asap` with the parallel stream
on `workers` threads. `"stream": "reference"` reproduces the reference program
exactly, on one thread. Each described partition becomes one ranked
hypothesis, and the candidate table goes to `<output>.asap.tsv`. `asap_import`
reads ASAP SPART-XML.
