# Tree support presentation and alignment association

Scientific support strings remain unchanged in Newick saves. Decimal precision
only affects canvas, SVG, tooltips and selectable tree text. Rust fixed-decimal
formatting uses nearest rounding, with ties to even. Each support set has its own
`support_decimals` entry in the tree sidecar's `style` (0–10 places); absent entries
use automatic defaults. Unknown style fields survive reading, editing and saving.

Each sidecar `supports` entry has a `kind`: `bootstrap`, `probability`,
`percentage` or `unknown` (legacy default). Known bootstrap/percentage values
use 0 places and percentage thresholds; known probability values use 2 places
and proportion thresholds. Metadata comes from known producers, not names.
Basecraft NJ/UPGMA bootstrap is named Bootstrap; FastTree's support is named
SH-like local support and uses probability scale. IQ-TREE currently requests no
support analysis. Imported `prob=` comments are known probability support and
retain their original numeric string.

For ambiguous numeric imports, automatic formatting treats a nonempty set with
all finite values in [0,1] as proportion style (2 places), otherwise percentage
style (0 places). This fallback does **not** establish a scientific support type.
Style's explicit scale (`support_proportions`) and decimal overrides resolve
ambiguous inputs without changing values or producer metadata. Low-valued known
bootstrap percentages always retain bootstrap semantics. Legacy Basecraft
distance reports identify bootstrap only when their recorded Newick result
matches the opened tree exactly. Unrelated imports remain unspecified.

Accepting Style records one `tree.style` operation. Optional `remove_supports`
(default empty for old logs) stages support removal until acceptance and undoes
with the other settings. Escape/Cancel discards the dialog's draft. Removing a
support set changes scientific content and is reported as an edit, not merely
presentation. Invalid removals/precision are rejected before recovery tracking.
`tree.add_support` retains the source's known `kind` in its replayable payload.

Generated trees use `.alignment.json` and a uniquely named `.alignment.afa` snapshot to retain scientific
source identity/hash, alignment generation and exact tool input/hash. Changing
the view association of an inferred tree creates `.view.alignment.json` pointing
to its own immutable snapshot; it never replaces the scientific origin or prior
retained bytes. Legacy fixed snapshot filenames remain readable. Save-as validates
both associations before replacing the tree, carries both snapshots, rebases
relative source references, and removes stale destination associations when the
saved document has none. During publication, unavailable metadata guards prevent
extraction or claims of the previous destination's scientific origin. On failure,
the guards remain and an adjacent `.alignment-recovery.json` record retains prior
metadata bytes/symlink targets and intended replacements. Prior snapshots remain
intact. Recovery records and unreferenced snapshots are not automatically deleted. Legacy absolute sources
remain readable. Keep these companions with the tree when moving a project.

Extraction checks snapshot hashes and exact tip identifiers (allowing additional
rows for inferred component trees). Missing/changed source files do not replace
retained input; damaged snapshots or unmatched tip names cause an actionable
error. An unreadable explicit view link never falls back to inference input.
Scientific tip renaming does not rename snapshot rows. Clade annotations do not
change scientific tip identity, support strings or inference provenance.

Scale-bar labels strictly greater than 0.001 use ordinary decimal notation,
including values at least 1000; smaller values keep scientific notation. Decimal
formatting uses a round-trip representation so small nonzero values cannot be
rounded misleadingly to zero.

`style.tip_labels` optionally maps original tip identities to display strings.
It changes canvas, figure export, tooltips and the selectable text display-label
column; Newick names, alignment matching and inference inputs retain their IDs.
Empty/absent maps preserve the normal name display. Metadata composition is an
explicit `tree.style` snapshot: `style.metadata_labels` retains ordered fields,
resolved specimen IDs, per-tip link issues and the canonical metadata hash.
Later metadata edits do not silently replace an accepted snapshot. Recompose
from Tree style > Labels to update it, or choose Original labels to clear it.
The `genus`, `specificEpithet` and `accession` columns supply the corresponding
fields; missing values are skipped. Unknown/duplicate record links keep the
original identity and report an issue. Taxon names are not split heuristically.

Project partitions are `Hypothesis` values in the metadata JSON manifest, with
members in the canonical specimen roster. Current identifications remain a
partition derived from recorded assignments. Explicit workspace links are stored
in each existing portable specimen attribute `basecraft_roster_id`, edited with
`delimitation.edit/set_roster_links`; absent links leave legacy workspaces usable.
Promotion requires complete valid links and rejects conflicting many-to-one
assignments. The promoted partition stores exact source hypothesis, mappings and
source/metadata hashes. Record-level fitted scores stay in the source evidence,
rather than being misrepresented as per-specimen scores after projection.

Headless clients use `basecraft metadata consumer REQUEST --project PROJECT`
(or MCP `metadata_consumer`, Python `Session.metadata_consumer`). Requests use
`action: partitions`, `labels` (document, records, fields), `link_workspace`
(workspace, links) or `promote_partition` (workspace, hypothesis, id). Planning
writes nothing. Edit plans return a target and normal typed operation; CLI
`--apply` applies and saves, while MCP/Python use version-checked apply and save.
