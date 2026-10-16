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
  --cache genes.distances.json --matrix genes.tsv --tree genes.nwk

# Reuse the observed cache with a different tree method.
basecraft-distance --cache genes.distances.json --tree genes.nwk --method njstar

# Group assignments are a JSON object, e.g. {"sample1":"speciesA","sample2":"speciesA"}.
basecraft-distance --cache genes.distances.json \
  --groups assignments.json --summary groups.summary.json
```

All outputs are optional; choose at least one. `--cache` alone calculates distances
without constructing a tree. With `--input`, an existing cache is reused only when
normalized alignment content, names, model and minimum overlap match; otherwise it
is replaced. Without `--input`, the cache's model and overlap settings are used.
Cache JSON writes are atomic. Matrix export uses `NA`; JSON uses `null` for missing
estimates. FASTA identifiers are the first header word and must be unique.

`--threads` controls distance calculation (default available CPUs capped at 16).
`--tree-threads` defaults to at most four. `--min-overlap` defaults to 1; choose a
larger value when short comparisons should be unavailable. Gaps and ambiguity
symbols are excluded pairwise; U is treated as T. Undefined JC69/K2P corrections
are missing, never zero. Protein correction models are not implemented.

## Trees and missing data

- `sorted`: optimized exact sorted-row NJ, with an exhaustive fallback when pruning
  is poor. Default; requires complete distances within each connected component.
- `fourpoint`: experimental four-point completion followed by sorted-row NJ; errors
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
  --cache genes.nwk.distances.json --model jc69 --groups assignments.json
```

The Basecraft command and GUI record the existing typed `tool.run` operation,
parameters, and hashes of all outputs. Group assignments passed through the
Basecraft command are captured as JSON parameters for reproducibility. No protocol
schema changes or private application hooks are required. Matrix viewing and a
GUI for editing group assignments remain separate taxonomy-workflow features.

The library exposes `Alignment`, `DistanceMatrix::calculate`, cache read/write,
`export_tsv`, `tree`, `groups`, and `bootstrap`. It has no Qt dependency.
Bootstrap cancellation is available through a public cancellation-file parameter
and the GUI Cancel bootstrap button, checked between replicates. Cache JSON prioritizes interoperability over large-scale storage
compression and can be much larger/slower to write than a tree alone.

## Verification

```sh
cargo test -p basecraft-distance
cargo build -p basecraft-distance
BASECRAFT_DISTANCE_TEST_BINARY=/absolute/path/to/basecraft-distance \
  cargo test -p basecraft-plugin-host --test distance_plugin
```

Tests cover model formulas and saturation, scalar/packed agreement, thread
reproducibility, exhaustive NJ objective checks, cache invalidation, missing-value
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
