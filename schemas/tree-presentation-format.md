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

Generated trees use `.alignment.json` and `.alignment.afa` to retain scientific
source identity/hash, alignment generation and exact tool input/hash. Changing
the view association of an inferred tree creates `.view.alignment.json` and
`.view.alignment.afa`; it never replaces the scientific origin. Save-as carries
both snapshots and rebases relative source references. Legacy absolute sources
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
