# Delimitation input and display choices

| Entry point | Analysis input | Deduplication | Display tree |
| --- | --- | --- | --- |
| Alignment GMYC, single/multiple | Equal-rate UPGMA on retained rows; uncalibrated relative time | Explicit on/off; default on | Fitted representative tree, with mapped specimen selection |
| Existing-tree GMYC | User-asserted rooted, validated ultrametric time tree | Off uses input unchanged; on is blocked with guidance | Existing fitted tree |
| PTP/mPTP | User-asserted rooted, bifurcating substitution tree | Off uses input unchanged; on is blocked with guidance | Existing fitted tree |
| ASAP | Aligned DNA, upstream ASAP | Explicit on/off; default off | None, UPGMA, NJ or IQ-TREE; all original rows, display only |
| Manual / display setup | User edits, no inferred partition | Not applicable; retains specimen roster | None, existing, UPGMA, NJ or IQ-TREE |
| Combine | Saved hypotheses with identical specimen rosters | Not applicable; does not rerun analyses | Existing workspace presentation |

The advanced tools use the same backend and an explicit sampling choice.
A precomputed representative tree and mapping can still be imported there.
Requesting automatic tree-only deduplication fails before a fit: this version
has no verified alignment-to-tree pruning workflow. Neither zero branches nor
similar labels establish sequence identity. Use the alignment path for GMYC or
ASAP, or prepare a representative tree and mapping before fitting. PTP inputs
must be explicitly rooted; IQ-TREE inference here does not root or date trees.

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
