# Delimitation input and display choices

| Entry point | Analysis input | Deduplication | Display tree |
| --- | --- | --- | --- |
| Alignment GMYC, single/multiple | Equal-rate UPGMA on retained rows; uncalibrated relative time | Explicit on/off; default on | Fitted representative tree, with mapped specimen selection |
| Existing-tree GMYC | Rooted (tag, two clades at the base, or stated) and ultrametric within the rounding of its written lengths; tips evened out before the fit and recorded. Any branch-length meaning; a tree that is not ultrametric is refused with the offer to smooth it (penalised likelihood) | Off uses input unchanged; on is blocked with guidance | Existing fitted tree |
| PTP/mPTP | Rooted (tag, two clades at the base, or stated), bifurcating substitution tree | Off uses input unchanged; on is blocked with guidance | Existing fitted tree |
| ASAP | Aligned DNA, upstream ASAP | Explicit on/off; default off | None, UPGMA, NJ or IQ-TREE; all original rows, display only |
| Manual / display setup | User edits, no inferred partition | Not applicable; retains specimen roster | None, existing, UPGMA, NJ or IQ-TREE |
| Combine | Saved hypotheses with identical specimen rosters | Not applicable; does not rerun analyses | Existing workspace presentation |

The advanced tools use the same backend and an explicit sampling choice.
A precomputed representative tree and mapping can still be imported there.
Requesting automatic tree-only deduplication fails before a fit: this version
has no verified alignment-to-tree pruning workflow. Neither zero branches nor
similar labels establish sequence identity. Use the alignment path for GMYC or
ASAP, or prepare a representative tree and mapping before fitting. A tree with
a `[&R]` or `[&U]` tag is read as stated; without one, two clades at the base
read as rooted and three or more as unrooted, a guess the run records (D18).
IQ-TREE inference here does not root or date trees.

Ultrametric (D17): each written branch length may be off by half a unit in its
last decimal, and a tip's allowed difference is that bound summed along its
root-to-tip path (lengths basecraft computed use the floating-point bound).
A tree within it has its tip branches evened out to the common depth that
changes the largest tip branch least; the run records the depth and change.
The tree view's threshold cut, time cuts and GMYC use this one check.

Exact identity compares whole aligned DNA strings after uppercase conversion
and U→T. All other accepted IUPAC codes, `?`, `-` and `.` are literal: N differs
from ?, gaps at different sites differ, and `.` differs from `-`. Rectangular,
nonempty alignments and unique nonempty IDs are required. Representatives are
the lexicographically first specimen IDs, with deterministic sorted membership.
ASAP currently rejects `.` as an unsupported analysis symbol on both sampling
paths. This restriction does not change the identity definition.

A fit uses each retained row once; duplicate multiplicities are not likelihood
weights. Expansion restores each specimen once, including unassigned members
and individual scores. Provenance retains the original alignment snapshot/hash,
sampling choice, counts, mapping/hash, effective analysis parameters and all
public plugin stages. The mapping is recomputed against retained evidence before
accepting a result. Changed snapshots or representative alignments are errors.
Deduplication off never silently becomes on: duplicate zero-time tips may cause
GMYC to fail and must be resolved explicitly by the user.

IQ-TREE display inference exposes its model (default MFP), seed and worker budget.
The public plugin records requested/selected model, version, command, input and
output evidence. `-keep-ident` retains all display rows. Display inference does
not change ASAP assignments. Missing tools, cancellation and mismatched output
labels fail without accepting a new completed tree or starting a pending fit.
IQ-TREE requires at least four rows through the current public plugin.

IQ-TREE lengths represent substitutions per site, not elapsed time. An IQ-TREE
display is not enabled for GMYC fitting or manual time cuts. UPGMA's equal-rate
assumption is not empirical clock calibration. No dating algorithm or automatic
rooting was added. See the IQ-TREE [command reference](https://iqtree.github.io/doc/Command-Reference)
for `-keep-ident` and [FAQ](https://www.iqtree.org/doc/Frequently-Asked-Questions)
for branch units and default handling of identical sequences.
