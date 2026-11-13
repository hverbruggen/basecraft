# Automatic tree rooting

**Midpoint** roots halfway along a longest leaf-to-leaf path, using branch
lengths. **MRTTV** minimizes the unweighted population variance of distances
from the root to **all** leaf tips, globally over nodes and edge interiors.
Neither method estimates dates, validates a molecular clock, nor makes an
arbitrary input ultrametric. Both operate on the current whole tree and are
undoable document edits; selection does not affect them.

The formulation follows Mai, Sayyari & Mirarab (2017), *Minimum variance
rooting of phylogenetic trees and implications for species tree
reconstruction*, PLOS ONE 12(8):e0182238,
<https://doi.org/10.1371/journal.pone.0182238>, definition of global MV and
Proposition 6. MRTTV here is this minimum-variance objective, not a regression
against sampling dates and not a minimum of mean distance.

## Calculation

Midpoint uses two iterative farthest-tip traversals and walks back along the
diameter to half its length. MRTTV computes the count, mean and centered sum
of squares (M2) on both sides of every edge. Prefix/suffix merges compute the
outside moments without subtracting nearly equal global sums. At distance
`x` from a node toward its parent, distances on the node side increase by
`x` and those on the other side decrease by `x`. Within-side M2 stays fixed.
The between-side contribution is

`n_left * n_right / n_total * (mean_left - mean_right + 2*x)^2`.

Thus each edge minimum is at
`clamp((mean_right - mean_left) / 2, 0, edge_length)`.
Comparing the merged variance at every edge minimum gives the global result,
including endpoints. Lengths are divided by the largest input edge before
calculating moments; original units are used in the resulting document.
No squared raw mean is subtracted, and there is no all-pairs distance matrix.

Traversal starts at the lexicographically smallest unique tip, with child
components ordered by their smallest tip. Equal farthest distances choose
the smallest tip name. MRTTV candidates within `64 * f64::EPSILON` relative
variance retain the first candidate in that canonical traversal. The metric
root is independent of the original root and child order within floating-point
roundoff. Zero-length connected positions can represent the same metric root.
Rendering order need not match the previous order.

The core traversals and rebuild use O(nodes) time and memory. Canonical sorting
costs O(nodes log nodes) worst case. Support storage adds O(nodes * support
sets). Annotation handling uses the existing tip-reference resolver; many
annotations on very deep trees can be more expensive. Parsing, rooting and
Newick serialization are iterative, including the 12,001-tip comb regression.

## Inputs and metadata

- At least two uniquely named tips and a finite nonnegative length on every
  non-root edge are required. Repeated names, missing/negative/nonfinite
  lengths and all-zero trees produce useful errors without edits or history.
- Polytomies, zero-length individual edges, two-tip trees and internal unary
  nodes are supported. A unary root is rejected because its stem has no tip
  on the other side. The root's incoming length is not an edge of the tree.
- Positive lengths whose smallest/largest ratio is below `1e-100`, or an
  overflowing merged root branch, are rejected rather than silently rounded
  into a different tree. Uniform scales of `1e-200` and `1e200` are tested.
- An unlabelled degree-two old root is suppressed if it has no root annotation
  and its two incoming support vectors have no conflicting nonempty values.
  Lengths are added and compatible support values combined. Named roots,
  annotated roots and roots with conflicting values are retained as vertices,
  possibly unary in the new orientation. Their metadata is not discarded.
- At an internal endpoint the existing vertex becomes the root. A tip endpoint
  retains the leaf identity via a zero-length root branch. Splitting an edge
  divides its length and stores support and branch labels **once** on one
  child branch, preferentially an internal side (the first stored endpoint
  if both are internal). Both pieces represent the same unrooted split; this
  is one annotation, not independent evidence for two branches.
- Support follows undirected branches. Branch-only clade annotations are
  remapped to the corresponding new child clade. Bars and fields retain their
  original tip-based MRCA references, as in the existing annotation model;
  rerooting can change the descendants of that MRCA. An annotation combining
  a branch label with a bar/field is rejected if those targets would diverge.
- A branch annotation that cannot name its new unary node with the existing
  one/two-tip reference is rejected. Placing support on a tip or a named
  internal node is also rejected because the current Newick representation
  cannot save both the name and support. This check covers every rebuilt
  branch, including a pendant edge formed by suppressing the old root.
  For example, `(A:1,(B:10,C:1)90:1);` is refused by both automatic methods:
  suppression would leave support 90 on tip A. No support is dropped or moved
  to another split, and no artificial carrier is inserted. Such refusals
  leave the document, undo/redo, history and recovery tracking unchanged.
- Multiple support sets preserve missing-value `-` placeholders on reload.
  Generic source Newick comments that the existing importer does not model
  are outside the rooting metadata contract.

The ribbon enables automatic commands for an open, idle tree with at least
two tips and some lengths. Full scientific validation occurs on invocation,
so a partially specified tree receives an explanatory error. Manual rerooting
retains its existing behavior.

## Validation and provenance

Typed operations `tree.midpoint_root` and `tree.minimum_variance_root` record
the method against the exact current document, with existing session hashes,
undo snapshots, save provenance and replay. They have no user parameters.
Their output is rooted: it is written with `[&R]` and read as rooted by the
user's action (D18), whatever the input declared. Split root branches are
computed lengths, so they carry no written precision.

The asymmetric star `(A:1,B:3,C:6);` has midpoint 1.5 units down the C edge
(depths 2.5, 4.5, 4.5) and MRTTV 2 units down that edge (depths 3, 5, 4).
Tests independently evaluate all tip distances at each edge's two endpoints
and midpoint, interpolate the variance quadratic, and compare the global
minimum on 40 deterministic irregular trees. Further tests cover pairwise
distances, unrooted splits and support, old-root changes, ties, metadata,
endpoint/zero cases, scales, rejection, undo/redo, save/reload and replay.
