# Basecraft distance engine

Rust library and standalone `basecraft-distance` CLI, also used by the bundled
`plugins/distance` public plugin. DNA models: **p**, **jc69**, **k2p**.
JC69 uses `-3/4 ln(1 - 4p/3)` with p the observed mismatch proportion.
K2P estimates transition and transversion proportions from each pair; no preset ratio.

## Install

From this repository, with build artifacts outside the source tree:

```sh
just install-distance
```

This builds and installs `basecraft-distance` into `~/.local/bin`; the repository's
Cargo configuration keeps build artifacts outside the source tree. Put that directory on PATH, or
set `BASECRAFT_DISTANCE` to its absolute path for the Basecraft plugin. Install the
bundled `plugins/distance` directory with Basecraft's other plugins. Development
builds find it in this repository; packaged builds use `BASECRAFT_PLUGINS`.

## Standalone use

```sh
# Calculate once; save observed distances, an optional TSV matrix and an NJ tree.
basecraft-distance --input genes.aln.fasta --model jc69 \
  --cache genes.distances.bin --matrix genes.tsv --tree genes.nwk

# Reuse the observed cache with a different tree method.
basecraft-distance --cache genes.distances.bin --tree genes.nwk --method njstar

# Group assignments are a JSON object, e.g. {"sample1":"speciesA","sample2":"speciesA"}.
basecraft-distance --cache genes.distances.bin \
  --groups assignments.json --summary groups.summary.json
```

All outputs are optional; choose at least one. `--cache` alone calculates distances
without constructing a tree. With `--input`, an existing cache is reused only when
normalized alignment content, names, model and minimum overlap match; otherwise it
is replaced. Without `--input`, the cache's model and overlap settings are used.
Cache writes are atomic. A cache is compact binary with a checksum unless its path ends in
`.json`, which writes the older readable JSON form; both load. Matrix export uses `NA` for missing
estimates, JSON uses `null`. FASTA identifiers are the first header word and must be unique.

`--threads` controls distance calculation (default available CPUs capped at 16).
`--tree-threads` defaults to at most four. `--min-overlap` defaults to 1; choose a
larger value when short comparisons should be unavailable. Gaps and ambiguity
symbols are excluded pairwise; U is treated as T. Undefined JC69/K2P corrections
are missing, never zero. Protein correction models are not implemented.

## Trees and missing data

- `sorted`: exact NJ, the default. It requires complete distances within each connected
  component. The search now uses cached row bounds rather than sorted rows, and chooses
  the same joins as an exhaustive search in the same arithmetic, ties included. The name stays so saved
  settings and history keep their meaning.
- `fourpoint`: experimental four-point completion followed by exact NJ; errors
  if completion cannot fill a connected component.
- `njstar`, `mvrstar`: experimental direct incomplete-distance methods. NJ* dispatches
  to the optimized search when the component is already complete. MVR* uses
  overlap-aware delta-method variances with a finite-site floor.

Disconnected components produce separate Newick trees; direct methods also retain
unresolved subtrees separately. No edges are invented between forest members.
Negative NJ limbs are retained. NJ is unrooted; UPGMA is rooted and
ultrametric. Support estimation is opt-in. These are independent implementations; the incomplete-data
variants and their accuracy limits are described in
[the research report](../../methods_research/parallel_nj/ROUND2.md).

## Where the methods come from

Basecraft's distance engine combines ideas published and implemented by others. All code
here was written for Basecraft; no source was copied from these programs. A run's generated
Methods text cites the methods that run used. Programs that were only references or
comparators are credited here and not in that text.

**Distance matrix**

- Counting sites with word operations comes from Elias & Lagergren (2007), who showed this
  removes the distance bottleneck, including the XOR split of transitions and transversions
  on a two-bit base code. Simonsen & Pedersen (2011) developed vectorized estimators, used
  with RapidNJ, and their gap filter is the source of the per-sequence filter for usable
  sites.
- DecentTree (Wang et al. 2023), FastME 2.0 (Lefort et al. 2015) and fastphylo (Khan et al.
  2013) were consulted as working fast implementations. DecentTree's source handles unknown
  characters differently, without a stored mask.
- Corrections use Jukes & Cantor (1969) and Kimura (1980).
- Basecraft's implementation choices are three stored bit planes giving p, JC69 and K2P from
  one count, short runs of partner rows that keep comparisons in cache, and kernels chosen
  at run time. It doesn't implement the ambiguity estimator of Elias & Lagergren: ambiguous
  sites are excluded pairwise.

**Tree inference**

- Neighbor joining is Saitou & Nei (1987), in the form given by Studier & Keppler (1988).
- Caching each row's smallest Q and rescanning only rows that could hold the next minimum
  is dynamic neighbor joining, by Clausen (2023), implemented in CCPhylo. Keeping one
  triangular matrix follows the same paper. Measured on real alignments, this is where the
  tree-step speedup comes from: on Culicidae COI with 1,000 sequences, join-score
  evaluations fell from 154 million to 12 million.
- Earlier versions of this engine used the sorted-row bound of RapidNJ (Simonsen et al.
  2008, 2011). RapidNJ and DecentTree remain the main comparators.
- The guard is Basecraft's own addition. Clausen's exactness argument assumes the triangle
  inequality and non-negative distances, which corrected and pairwise-deleted distances can
  break. Tracking how far a join can lower each row's Q keeps the bound valid for any finite
  matrix small enough not to overflow. It comes from the October 2026 prototypes. We know of
  no published equivalent, though our search for one was limited, so it should be cited as
  this software unless an earlier source turns up.
- Missing distances use NJ* and MVR* (Criscuolo & Gascuel 2008), or a synchronous, uncapped
  variant of four-point completion (Landry et al. 1996; Makarenkov & Lapointe 2004), without
  the weighted least-squares refinement of MW*. UPGMA is Sokal & Michener (1958), with the
  nearest-neighbor chain surveyed by Murtagh (1983). Bootstrap is Felsenstein (1985), using
  the SplitMix64 generator (Steele et al. 2014).

**What "exact" means here.** Joins match an exhaustive search in the engine's own
double-precision arithmetic, including how ties break. On data with many exact ties, such as
barcodes, rounding settles which of several equally good joins is chosen, so another correct
NJ program can return a different tree with the same claim to being NJ.

**References**

- Clausen PTLC. 2023. Scaling neighbor joining to one million taxa with dynamic and heuristic neighbor joining. Bioinformatics 39:btac774. https://doi.org/10.1093/bioinformatics/btac774
- Criscuolo A, Gascuel O. 2008. Fast NJ-like algorithms to deal with incomplete distance matrices. BMC Bioinformatics 9:166. https://doi.org/10.1186/1471-2105-9-166
- Elias I, Lagergren J. 2007. Fast computation of distance estimators. BMC Bioinformatics 8:89. https://doi.org/10.1186/1471-2105-8-89
- Felsenstein J. 1985. Confidence limits on phylogenies: an approach using the bootstrap. Evolution 39:783-791.
- Jukes TH, Cantor CR. 1969. Evolution of protein molecules. In: Munro HN, editor. Mammalian Protein Metabolism. New York: Academic Press. p 21-132.
- Khan MA, Elias I, Sjölund E, et al. 2013. Fastphylo: fast tools for phylogenetics. BMC Bioinformatics 14:334. https://doi.org/10.1186/1471-2105-14-334
- Kimura M. 1980. A simple method for estimating evolutionary rates of base substitutions through comparative studies of nucleotide sequences. J Mol Evol 16:111-120.
- Landry PA, Lapointe FJ, Kirsch JAW. 1996. Estimating phylogenies from lacunose distance matrices: additive is superior to ultrametric estimation. Mol Biol Evol 13:818-823. https://doi.org/10.1093/oxfordjournals.molbev.a025642
- Lefort V, Desper R, Gascuel O. 2015. FastME 2.0: a comprehensive, accurate, and fast distance-based phylogeny inference program. Mol Biol Evol 32:2798-2800. https://doi.org/10.1093/molbev/msv150
- Makarenkov V, Lapointe FJ. 2004. A weighted least-squares approach for inferring phylogenies from incomplete distance matrices. Bioinformatics 20:2113-2121. https://doi.org/10.1093/bioinformatics/bth211
- Murtagh F. 1983. A survey of recent advances in hierarchical clustering algorithms. Comput J 26:354-359. https://doi.org/10.1093/comjnl/26.4.354
- Saitou N, Nei M. 1987. The neighbor-joining method: a new method for reconstructing phylogenetic trees. Mol Biol Evol 4:406-425. https://doi.org/10.1093/oxfordjournals.molbev.a040454
- Simonsen M, Mailund T, Pedersen CNS. 2008. Rapid neighbour-joining. In: Algorithms in Bioinformatics, LNCS 5251:113-122. https://doi.org/10.1007/978-3-540-87361-7_10
- Simonsen M, Mailund T, Pedersen CNS. 2011. Inference of large phylogenies using neighbour-joining. In: Biomedical Engineering Systems and Technologies, CCIS 127:334-344. https://doi.org/10.1007/978-3-642-18472-7_26
- Simonsen M, Pedersen CNS. 2011. Rapid computation of distance estimators from nucleotide and amino acid alignments. Proc ACM Symposium on Applied Computing, 89-93. https://doi.org/10.1145/1982185.1982208
- Sokal RR, Michener CD. 1958. A statistical method for evaluating systematic relationships. Univ Kansas Sci Bull 38:1409-1438.
- Steele GL, Lea D, Flood CH. 2014. Fast splittable pseudorandom number generators. Proc OOPSLA, 453-472. https://doi.org/10.1145/2660193.2660195
- Studier JA, Keppler KJ. 1988. A note on the neighbor-joining algorithm of Saitou and Nei. Mol Biol Evol 5:729-731. https://doi.org/10.1093/oxfordjournals.molbev.a040527
- Wang W, Barbetti J, Wong T, et al. 2023. DecentTree: scalable Neighbour-Joining for the genomic era. Bioinformatics 39:btad536. https://doi.org/10.1093/bioinformatics/btad536

## Grouping statistics

Summary JSON contains specimen count, observed mean/maximum within-group distance,
minimum observed between-group distance and its group, comparison counts, and a
barcode-gap result. Unassigned specimens are counted and excluded from group
comparisons. Singletons have unavailable within-group statistics. A barcode-gap
claim is unavailable when relevant comparisons are missing. Ties between nearest
groups use lexical order. Tree completion never modifies the saved observed cache.

## Basecraft integration

The alignment ribbon's **Distance tree** group offers Matrix, NJ and UPGMA.
The settings popup holds the model, overlap, workers and bootstrap controls.
Work runs through the existing background public-plugin path, using NJ* for
incomplete components and optimized NJ for complete ones. Trees, distance cache and
run metadata are saved beside the alignment; forest components open separately.

```sh
basecraft distance genes.aln.fasta --output genes.nwk --model jc69
basecraft distance genes.aln.fasta --output genes.tsv --model jc69 --matrix
basecraft distance genes.aln.fasta --output groups.summary.json \
  --cache genes.nwk.distances.bin --model jc69 --groups assignments.json
```

The Basecraft command and GUI record the existing typed `tool.run` operation,
parameters, and hashes of all outputs. Group assignments passed through the
Basecraft command are captured as JSON parameters for reproducibility. No protocol
schema changes or private application hooks are required. Matrix viewing and a
GUI for editing group assignments remain separate taxonomy-workflow features.

The library exposes `Alignment`, `DistanceMatrix::calculate`, cache read/write,
`export_tsv`, `tree`, `groups`, and `bootstrap`. It has no Qt dependency.
Bootstrap cancellation is available through a public cancellation-file parameter
and the GUI Cancel bootstrap button, checked between replicates. JSON caches favour
inspection over size, so the plugin writes compact ones.

## Verification

```sh
cargo test -p basecraft-distance
cargo build -p basecraft-distance
BASECRAFT_DISTANCE_TEST_BINARY=/absolute/path/to/basecraft-distance \
  cargo test -p basecraft-plugin-host --test distance_plugin
```

Tests cover model formulas and saturation, scalar/packed agreement, every available CPU
kernel, thread reproducibility, byte-for-byte agreement with a separate exhaustive NJ on
metric, non-metric and tied matrices, cache invalidation and damage, missing-value
round trips, grouping changes, plugin provenance and operation-log replay.

## UPGMA

`--method upgma` uses size-weighted average linkage and a reciprocal
nearest-neighbor chain. It returns a rooted ultrametric tree (equal-rate
assumption), with deterministic ties and cluster heights equal to half their
merge distance. Distances remain unchanged. Disconnected complete components
produce separate trees; incomplete connected components fail explicitly.
Distance estimation is parallel; the UPGMA chain is serial. The implementation
uses quadratic matrix storage and nearest-neighbor-chain work; it is checked
against an independent global-minimum average-linkage oracle.

```sh
basecraft-distance --input genes.afa --model k2p --method upgma --tree genes.upgma.nwk
basecraft distance genes.afa --method upgma --output genes.upgma.nwk
```

## Ordinary site bootstrap

```sh
basecraft-distance --input genes.afa --model k2p --method sorted \
  --tree genes.bootstrap.nwk --bootstrap 1000 --seed 71 --report genes.run.json
basecraft distance genes.afa --output genes.upgma.nwk --method upgma \
  --bootstrap 1000 --seed 71
```

Bootstrap is off by default. Enabling it in the GUI suggests 1,000 replicates;
Matrix export ignores these settings. Only complete-matrix sorted NJ and UPGMA
are supported. The exact aligned FASTA is required even with a reusable distance
cache. Every replicate samples exactly L sites with replacement using one shared
column-multiplicity vector, with unbiased SplitMix64 sampling (`splitmix64-rejection-v1`).
The replicate index and seed determine that vector independently of worker counts.
Counts of valid sites, transitions and transversions precede p/JC69/K2P correction.
Pairwise deletion permits gaps and ambiguity; it can make a replicate undefined.

The engine first infers the original tree, then annotates its original branch
lengths with bootstrap percentages: canonical **unrooted splits** for NJ and
**rooted descendant clades** for UPGMA. It infers every replicate tree afresh;
there is no fixed candidate-tree search, early stopping, or ultrafast approximation.
Trivial tip splits and the root have no support labels. A replicate split counts
at most once even if a binary Newick root represents both sides of the same edge.

Any undefined original distance prevents inference and records its pair/cause.
Any undefined or failed replicate withholds **all** support and publishes the
original tree without labels. Replicates are never skipped or replaced; successful
runs report requested, attempted, completed and failed counts. Diagnostics retain
up to 20 individual failures and the exact total count. Cancellation also withholds
support and retains the original tree. Reports identify method, seed, kernel,
counts, support convention, failure causes and distance/tree/total timings.
The public plugin saves and hashes reports through `tool.run`, including runs
with failed replicates. Original-inference failures retain a diagnostic report,
but the current host does not log failed tool invocations as `tool.run`.

The GUI's **Save bootstrap trees in** choice and the application CLI's
`--bootstrap-destination metadata|working` select a supporting collection in the
allocated metadata directory (default) or beside the primary output. The plugin
parameter is `bootstrap_destination`, also available to SDK callers. Host-managed
runs use the public `analysis_artifacts` object with absolute existing
`metadata_directory`, `working_directory`, and ASCII alphanumeric/hyphen `run_id`.
The host reserves the run once, anchored to the final primary output. The wrapper
selects `bootstrap-<run_id>.nex`; it does not derive storage from its temporary
engine output path. Returned collections are registered, not copied or republished.

A direct plugin caller can instead supply an absolute `bootstrap_trees` path.
Combining that override with `analysis_artifacts` is an error. Bootstrap without
either allocation or an explicit path fails with a useful error. Runs without
bootstrap need no collection allocation. The standalone engine takes an explicit
path and does not allocate a hidden store:

```sh
basecraft-distance --input genes.afa --model p --method sorted \
  --tree genes.nwk --bootstrap 1000 --seed 71 \
  --bootstrap-trees genes.replicates.nex --report genes.run.json
```

`basecraft-distance --bootstrap-trees PATH` retains the actual successful
replicates in a NEXUS TREES block. Each `TREE replicate_N` uses the original
one-based attempt number, taxon names and inferred branch lengths. Records stream
in attempt order; retention does not accumulate a forest in memory, resample
again, change support, or make another input copy. Existing destination files
are never replaced.

The report's `bootstrap.replicate_trees` gives the published path, format, status
and tree count. Complete collections use the requested path; withheld runs use
`<stem>.partial.nex`. The file footer records requested, attempted, retained and
failed counts. Status is `complete`, `partial`, `cancelled`, or `original_failed`.
Failed attempts leave gaps in tree numbers; diagnostics remain in the run report.
Zero-success partial collections contain an empty TREES block and counts, so
there is no tree to open. Cancellation is checked between replicates as before.
An I/O error aborts retention, removes only the writer's disposable temporary file,
and never advertises it as a complete collection. Atomic collection publication
and report/primary-tree writes are separate operations; an error after publication
can leave the collection recoverable at its destination.

If cache, primary tree, matrix, summary, final progress or report writing fails
after collection publication, the native CLI exits nonzero but still emits JSON
with `output_status: "failed"`, an `error`, and the actual published collection's
path/status/count under `bootstrap.replicate_trees`. A complete collection can
therefore belong to a failed output operation; its scientific status is preserved.
The wrapper validates that evidence against the requested destination and counts,
then saves a failed recovery report when possible and includes the evidence in its
error. Scratch creation/cleanup errors take the same recovery path. It never
claims ownership of a collection merely because a file exists at the expected
path. The library exposes `BootstrapPublicationError.report` when final progress
writing fails after publication; other errors before publication carry no retained
collection claim.

NEXUS block boundaries are recognized as commands outside quoted names and nested
comments, preserving taxa such as `weekend;sample` when reopening collections.

IQ-TREE currently requests no support replicates; FastTree reports its SH-like
local support. Neither wrapper produces an ordinary bootstrap-tree collection,
and this option does not change those methods.

`--progress PATH` atomically updates bootstrap status JSON; creating
`--cancel-file PATH` requests cancellation between replicates. The public plugin
parameters are `progress` and `cancel_file`. The GUI owns a unique temporary job
directory, polls it off the UI thread and exposes progress and cancellation;
stale job updates are discarded and cleanup waits for the worker to finish.
An individual distance/tree calculation is not interrupted mid-replicate.

Packed multiplicity bitplanes avoid an O(n² × patterns) pair-category cache.
A second exact kernel explicitly expands the shared weights one row at a time
and packs the replicate for ordinary popcount counting. Release benchmarks across
24–1,024 taxa and 2,000–100,000 columns justify a conservative deterministic
crossover: explicit packing for n≥256 or n≥128 with L≤4,096, weighted bitplanes
otherwise. This heuristic affects runtime only and is not a universal performance
guarantee. Source/alignment storage is O(nL), packed rows O(nL/64), and distance/tree
storage O(n²); there is no storage proportional to replicate count times novel
splits. Only original splits accumulate frequencies. See the retained
[benchmark script](../../methods_research/parallel_nj/benchmark_bootstrap.py) and
[results](../../methods_research/parallel_nj/bootstrap-production-results.json).

Tests compare integer counts, corrected distances, variances and inferred trees
against explicit resampling through the original packed engine for all three
models, gaps, ambiguity and U=T. An ordinary-resampling oracle verifies fractional
support; tests also check rooting conventions, branch lengths, escaped names,
worker independence, undefined distances, cancellation and public CLI reports.
The historical Python feasibility investigation remains separate in the
[Phase 1A decisions](../../docs/decisions/0009-phase1a-workflows.md).
Synthetic correctness/timing evidence is not biological support calibration.
