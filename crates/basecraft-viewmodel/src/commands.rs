//! The desktop app's commands: each command's caption, tip, access key and
//! the ribbon scope it belongs to, its icon, why it is unavailable in a given state,
//! and which ribbon tabs a document kind shows. The ribbon, the shortcuts
//! dialog and key handling read this one list.

pub struct Command {
    pub id: &'static str,
    /// The ribbon or menu caption.
    pub caption: &'static str,
    /// The tooltip while the command is available.
    pub tip: &'static str,
    /// Access key within its scope, shown on the key hint overlay.
    pub key: &'static str,
    /// The ribbon tab or menu the key belongs to.
    pub scope: &'static str,
    /// An icon chosen for this command; others follow from its id.
    pub icon: &'static str,
}

pub static COMMANDS: &[Command] = &[
    Command {
        id: "species.combine-",
        caption: "Combine",
        tip: "",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "species.manual-",
        caption: "Manual",
        tip: "",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "species.ptp-",
        caption: "PTP",
        tip: "",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "species.asap-",
        caption: "ASAP",
        tip: "",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "species.gmyc-",
        caption: "GMYC",
        tip: "",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "alignment.blast",
        caption: "BLAST",
        tip: "Submit selected or all FASTA rows to NCBI",
        key: "B",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "analysis.distance.matrix",
        caption: "",
        tip: "",
        key: "M",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "analysis.distance.nj",
        caption: "",
        tip: "",
        key: "N",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "analysis.distance.upgma",
        caption: "",
        tip: "",
        key: "U",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "application.preferences",
        caption: "Preferences…",
        tip: "Appearance, files, compute, ribbon and shortcuts",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "export.zenodo",
        caption: "Zenodo",
        tip: "",
        key: "U",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "figure.export-png",
        caption: "Export PNG",
        tip: "Export a PNG image of this view",
        key: "EX",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "file.new-sequence",
        caption: "New",
        tip: "New sequence",
        key: "N",
        scope: "File",
        icon: "",
    },
    Command {
        id: "file.open",
        caption: "Open…",
        tip: "Open a sequence, alignment, trace, tree, review, species workspace, specimen table or submission",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "file.recover-work",
        caption: "Recover work…",
        tip: "Reconstruct saved recovery journals into a new folder",
        key: "RE",
        scope: "File",
        icon: "",
    },
    Command {
        id: "file.save",
        caption: "Save",
        tip: "Save the current file or trace review",
        key: "S",
        scope: "File",
        icon: "",
    },
    Command {
        id: "file.save-as",
        caption: "Save as…",
        tip: "Save as a new file and carry on in that one",
        key: "A",
        scope: "File",
        icon: "",
    },
    Command {
        id: "help.about",
        caption: "About Basecraft…",
        tip: "Version and licence",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "help.context-shortcuts",
        caption: "Shortcuts…",
        tip: "Keyboard shortcuts for this tab",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "help.learn",
        caption: "Learn",
        tip: "Manual pages are not written yet",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "help.shortcuts",
        caption: "Keyboard shortcuts…",
        tip: "List all keyboard shortcuts",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "metadata.import-headers",
        caption: "Headers",
        tip: "",
        key: "H",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "metadata.import-sheet",
        caption: "Spreadsheet",
        tip: "Import specimen data from .ods, .xlsx, CSV or TSV",
        key: "SS",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "metadata.table",
        caption: "Table",
        tip: "",
        key: "TT",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "metadata.template",
        caption: "Template",
        tip: "",
        key: "TE",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "project.open",
        caption: "Open project…",
        tip: "Choose or create a project folder",
        key: "O",
        scope: "File",
        icon: "",
    },
    Command {
        id: "provenance.history",
        caption: "Provenance",
        tip: "Sources, analyses and edits for the current document",
        key: "P",
        scope: "File",
        icon: "",
    },
    Command {
        id: "provenance.methods",
        caption: "Methods",
        tip: "",
        key: "M",
        scope: "File",
        icon: "",
    },
    Command {
        id: "provenance.package",
        caption: "Package for reproduction…",
        tip: "Copy the result and retained history to a new package directory",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "provenance.show-files",
        caption: "Record folder",
        tip: "Open the saved result's readable provenance folder",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "provenance.verify",
        caption: "Verify…",
        tip: "Verify recorded input and output hashes without executing tools",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "ribbon.accept-this-base-and-go-to-the-next-issue",
        caption: "Accept",
        tip: "Accept this base and go to the next issue",
        key: "AA",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.add-all-sequences-from-a-fasta-file",
        caption: "FASTA",
        tip: "Add all sequences from a FASTA file",
        key: "F",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.add-support-values-from-another-analysis-of-the-same",
        caption: "Add",
        tip: "Add support values from another analysis of the same taxa, matched branch by branch",
        key: "AD",
        scope: "Tree",
        icon: "support-add",
    },
    Command {
        id: "ribbon.add-the-edited-consensus-to-its-alignment",
        caption: "Alignment",
        tip: "Add the edited consensus to its alignment",
        key: "AI",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.annotate-open-reading-frames",
        caption: "",
        tip: "",
        key: "O",
        scope: "Sequence",
        icon: "gene-orf",
    },
    Command {
        id: "ribbon.annotate-protein-coding-genes-with-prodigal",
        caption: "",
        tip: "",
        key: "PR",
        scope: "Sequence",
        icon: "gene-predict",
    },
    Command {
        id: "ribbon.annotate-the-selection",
        caption: "Add annotation",
        tip: "Annotate the selection; right-to-left selection defaults to reverse strand",
        key: "A",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.associate-an-imported-tree-with-an-alignment-after-validating",
        caption: "Link",
        tip: "Choose an alignment for viewing and selection. Identifiers are validated; the original analysis source is retained.",
        key: "LI",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.choose-alignment-context",
        caption: "Alignment",
        tip: "Choose alignment context",
        key: "AL",
        scope: "Trace",
        icon: "link",
    },
    Command {
        id: "ribbon.choose-the-consensus-range-to-keep-in-exports",
        caption: "Mask ends",
        tip: "Choose the consensus range to keep in exports",
        key: "M",
        scope: "Trace",
        icon: "trace-mask",
    },
    Command {
        id: "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft",
        caption: "Signal cutoff",
        tip: "Choose the terminal signal percentage for a new draft",
        key: "SS",
        scope: "Trace",
        icon: "trace-cutoff",
    },
    Command {
        id: "ribbon.combine-up-to-three-reads",
        caption: "Draft reads",
        tip: "Combine up to three reads",
        key: "D",
        scope: "Trace",
        icon: "trace-draft",
    },
    Command {
        id: "ribbon.copy-the-edited-consensus-as-fasta",
        caption: "Copy",
        tip: "Copy the edited consensus as FASTA",
        key: "CO",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.copy-the-selected-annotation-s-sequence",
        caption: "",
        tip: "",
        key: "C",
        scope: "Sequence",
        icon: "clipboard",
    },
    Command {
        id: "ribbon.delete-columns-containing-only-gaps-across-all-rows-including",
        caption: "",
        tip: "",
        key: "DD",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.delete-the-selected-annotation",
        caption: "",
        tip: "",
        key: "D",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.delete-the-selected-rows",
        caption: "Delete",
        tip: "Delete the selected rows",
        key: "DE",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.edit-the-selected-annotation",
        caption: "Edit",
        tip: "Edit the selected annotation",
        key: "EE",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.export-an-editable-svg-of-this-view",
        caption: "Export SVG…",
        tip: "Export an editable SVG of this view",
        key: "EE",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "ribbon.export-the-edited-consensus-as-fasta",
        caption: "Save",
        tip: "Export the edited consensus as FASTA",
        key: "SA",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.extract-aligned-rows-without-realigning-ctrl-click-to-select",
        caption: "Clade",
        tip: "Create clade alignment from selected clades without realigning; Ctrl-click to select multiple clades",
        key: "C",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.find-partial-terminal-primer-candidates-and-mask-them",
        caption: "Set primers",
        tip: "Find partial terminal primer candidates and mask them",
        key: "SE",
        scope: "Trace",
        icon: "trace-primer",
    },
    Command {
        id: "ribbon.fit",
        caption: "",
        tip: "",
        key: "F",
        scope: "View",
        icon: "",
    },
    Command {
        id: "ribbon.infer-a-tree-with-fasttree-standard-settings-gtr-for",
        caption: "",
        tip: "",
        key: "F",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "ribbon.insert-typing-inserts-bases-the-rest-of-the-row",
        caption: "",
        tip: "",
        key: "I",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.move-the-selected-rows-down",
        caption: "Down",
        tip: "Move the selected rows down",
        key: "DO",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.move-the-selected-rows-up",
        caption: "Up",
        tip: "Move the selected rows up",
        key: "U",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.name-the-selected-clade-with-a-bar-or-a",
        caption: "Annotate",
        tip: "Name the selected clade, with a bar or a coloured field",
        key: "AA",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.next-unresolved-issue",
        caption: "",
        tip: "Next unresolved issue",
        key: "N",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.order-every-clade-s-branches-by-size-largest-first",
        caption: "",
        tip: "",
        key: "L",
        scope: "Tree order menu",
        icon: "",
    },
    Command {
        id: "ribbon.order-every-clade-s-branches-by-size-smallest-first",
        caption: "",
        tip: "",
        key: "S",
        scope: "Tree order menu",
        icon: "",
    },
    Command {
        id: "ribbon.overwrite-typing-replaces-the-selected-bases",
        caption: "",
        tip: "",
        key: "O",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.previous-unresolved-issue",
        caption: "",
        tip: "Previous unresolved issue",
        key: "P",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.realign-the-sequences-with-mafft-default-settings-into-a",
        caption: "",
        tip: "",
        key: "M",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.rename-the-cursor-s-row",
        caption: "Rename",
        tip: "Rename the cursor's row",
        key: "R",
        scope: "Alignment",
        icon: "rename",
    },
    Command {
        id: "ribbon.reverse-the-order-of-the-branches-in-the-selected",
        caption: "Flip",
        tip: "Reverse the order of the branches in the selected subtree (for a tip, its parent's)",
        key: "F",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.review-chromatograms-and-add-their-consensus",
        caption: "Chromatogram",
        tip: "Review chromatograms and add their consensus",
        key: "C",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.root-the-tree-on-the-branch-above-the-selection",
        caption: "Reroot",
        tip: "Root the tree on the branch above the selection",
        key: "R",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.save-the-selection-as-a-new-file",
        caption: "",
        tip: "",
        key: "EX",
        scope: "Sequence",
        icon: "selection",
    },
    Command {
        id: "ribbon.select-a-model-and-infer-a-maximum-likelihood-tree",
        caption: "",
        tip: "",
        key: "I",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "ribbon.select-the-next-annotation",
        caption: "Next",
        tip: "Select the next annotation",
        key: "N",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.select-the-previous-annotation",
        caption: "Previous",
        tip: "Select the previous annotation",
        key: "PP",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.trim-unreliable-columns-with-trimal-automated1-into-a-new",
        caption: "",
        tip: "",
        key: "TT",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.type-a-new-sequence-or-paste-fasta",
        caption: "Type/paste",
        tip: "Type a new sequence or paste FASTA",
        key: "TY",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.which-support-values-show-circles-at-nodes-sizes",
        caption: "Style",
        tip: "Which support values show and how, and circles at nodes",
        key: "ST",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.zoom-in",
        caption: "",
        tip: "Zoom in",
        key: "I",
        scope: "View",
        icon: "",
    },
    Command {
        id: "ribbon.zoom-out",
        caption: "",
        tip: "Zoom out",
        key: "O",
        scope: "View",
        icon: "",
    },
    Command {
        id: "ribbon.zoom-to-the-selection",
        caption: "",
        tip: "",
        key: "S",
        scope: "View",
        icon: "",
    },
    Command {
        id: "sequence.blast",
        caption: "BLAST",
        tip: "Submit sequence queries to NCBI and open browser results",
        key: "B",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "species.asap-workspace",
        caption: "",
        tip: "",
        key: "AA",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.columns",
        caption: "Columns",
        tip: "Choose which hypotheses get a bar column",
        key: "CO",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.combine-project",
        caption: "Combine",
        tip: "Combine saved species hypotheses",
        key: "C",
        scope: "File",
        icon: "",
    },
    Command {
        id: "species.combine-workspace",
        caption: "",
        tip: "",
        key: "CC",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.details",
        caption: "Details",
        tip: "Species of the hypothesis being edited, fit report and distances",
        key: "D",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.display",
        caption: "Attach",
        tip: "",
        key: "AT",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.gmyc-workspace",
        caption: "",
        tip: "",
        key: "GG",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.large-on-top",
        caption: "",
        tip: "",
        key: "L",
        scope: "Species order menu",
        icon: "",
    },
    Command {
        id: "species.layout",
        caption: "Layout",
        tip: "Bar width, the gap between columns and the space between species, for every column",
        key: "L",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.manual-workspace",
        caption: "",
        tip: "",
        key: "M",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.new",
        caption: "New",
        tip: "New species hypothesis, empty or copied from another",
        key: "N",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.order-branches",
        caption: "",
        tip: "",
        key: "O",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.ptp-workspace",
        caption: "",
        tip: "",
        key: "P",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.roster-links",
        caption: "Roster",
        tip: "Explicitly link workspace records to project specimens and save partitions",
        key: "R",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.select-tool",
        caption: "Select",
        tip: "Click a node to select its clade, or a bar to select a species",
        key: "SE",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.small-on-top",
        caption: "",
        tip: "",
        key: "S",
        scope: "Species order menu",
        icon: "",
    },
    Command {
        id: "species.style-extra",
        caption: "",
        tip: "",
        key: "E",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.support-style",
        caption: "Support",
        tip: "Which support values show and how, and their size",
        key: "SS",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.text-view",
        caption: "Text",
        tip: "Each hypothesis's species and their sequences, as text",
        key: "TE",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.threshold-cut",
        caption: "Threshold",
        tip: "",
        key: "TH",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.tree-view",
        caption: "Tree",
        tip: "The tree with a bar column per hypothesis",
        key: "TT",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "submission.new",
        caption: "Start",
        tip: "",
        key: "ST",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "submission.open",
        caption: "Open",
        tip: "Open a GenBank submission of this project",
        key: "O",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "submission.prepare",
        caption: "Prepare",
        tip: "",
        key: "P",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "submission.submitter",
        caption: "Submitter",
        tip: "Your contact details for submission templates",
        key: "SU",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "submission.validate",
        caption: "Validate",
        tip: "Export the package and check it with table2asn on this computer",
        key: "V",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "supportFont",
        caption: "",
        tip: "",
        key: "SS",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tipFont",
        caption: "",
        tip: "",
        key: "T",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tools.installed-operations",
        caption: "Installed operations…",
        tip: "Choose a compatible installed operation; other schemas remain available through the API",
        key: "",
        scope: "",
        icon: "",
    },
    Command {
        id: "trace.cancel-calls",
        caption: "Cancel",
        tip: "Cancel background calling; keep the current review",
        key: "CA",
        scope: "Trace",
        icon: "trace-cancel",
    },
    Command {
        id: "trace.compute-calls",
        caption: "Compute",
        tip: "Compute or reuse alternative Tracy calls",
        key: "CC",
        scope: "Trace",
        icon: "trace-compute",
    },
    Command {
        id: "trace.keep-from",
        caption: "Keep from",
        tip: "Keep from the selected consensus column",
        key: "KF",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "trace.keep-through",
        caption: "Keep through",
        tip: "Keep through the selected consensus column",
        key: "KT",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "tree.metadata-labels",
        caption: "",
        tip: "",
        key: "LL",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.midpoint_root",
        caption: "Midpoint",
        tip: "Root halfway along a longest tip-to-tip branch-length path over the whole tree. Requires complete nonnegative lengths and unique tip names; does not infer dates or make the tree ultrametric.",
        key: "MM",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.minimum_variance_root",
        caption: "MRTTV",
        tip: "Minimize unweighted root-to-tip path length variance over ALL tips, globally across branch interiors and nodes. Requires complete nonnegative lengths and unique tip names; does not infer dates or make the tree ultrametric.",
        key: "MR",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.order-branches",
        caption: "",
        tip: "",
        key: "O",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.rate-smoothing",
        caption: "PL",
        tip: "Smooth rates on the current rooted tree with Sanderson penalized likelihood",
        key: "P",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.style-extra",
        caption: "",
        tip: "",
        key: "E",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "view.find",
        caption: "Find",
        tip: "Find names or sequence motifs in this document",
        key: "D",
        scope: "View",
        icon: "",
    },
    Command {
        id: "view.text",
        caption: "Text representation…",
        tip: "Read and copy the current scientific document as text",
        key: "T",
        scope: "View",
        icon: "",
    },
    Command {
        id: "zoomPercentage",
        caption: "",
        tip: "",
        key: "QM",
        scope: "View",
        icon: "",
    },
];

fn find(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.id == id)
}

/// A field of command `id`; an entry whose id ends in "-" supplies it for
/// every id it begins, such as the species methods offered per document kind.
fn field(id: &str, get: fn(&Command) -> &'static str) -> &'static str {
    let own = find(id).map_or("", get);
    if !own.is_empty() {
        return own;
    }
    COMMANDS
        .iter()
        .find(|command| command.id.ends_with('-') && id.starts_with(command.id))
        .map_or("", get)
}

pub fn caption(id: &str) -> &'static str {
    field(id, |command| command.caption)
}

pub fn tip(id: &str) -> &'static str {
    field(id, |command| command.tip)
}

pub fn key(id: &str) -> &'static str {
    find(id).map_or("", |command| command.key)
}

pub fn scope(id: &str) -> &'static str {
    find(id).map_or("", |command| command.scope)
}

/// The command's icon: its own, else one that its id suggests.
pub fn icon(id: &str) -> &'static str {
    if let Some(icon) = find(id)
        .map(|command| command.icon)
        .filter(|icon| !icon.is_empty())
    {
        return icon;
    }
    if id.is_empty() {
        return "info";
    }
    let has = |part: &str| id.contains(part);
    let any = |parts: &[&str]| parts.iter().any(|part| id.contains(part));
    if any(&["copy-", "extract-"]) {
        "selection"
    } else if any(&["export-", "save-", "package-"]) {
        "save-as"
    } else if any(&["annotation", "annotate-"]) {
        "annotation-add"
    } else if any(&["rename-", "edit-"]) {
        "edit"
    } else if any(&["link-", "associate-"]) {
        "link"
    } else if any(&["order-", "above-"]) {
        "up"
    } else if has("below-") {
        "down"
    } else if any(&["flip-", "reverse-"]) {
        "arrow-right"
    } else if any(&["tree", "phylogeny", "clade", "distance."]) {
        "tree"
    } else if any(&["mafft", "trimal", "alignment", "matrix"]) {
        "columns"
    } else if any(&["trace", "draft", "signal", "primer", "consensus"]) {
        "waveform"
    } else if any(&["find-", "predict-"]) {
        "bug"
    } else if has("open-") {
        "open"
    } else if has("move-") {
        "arrow-right"
    } else if has("add-") {
        "add"
    } else {
        "info"
    }
}

/// What the window shows now, as far as command availability depends on it.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // Independent facts the window reports.
pub struct Context {
    /// A project folder is chosen.
    pub project: bool,
    /// A document tab is active.
    pub document: bool,
    /// The active document has a file.
    pub path: bool,
    /// The active tab's kind: "sequence", "alignment", "tree", "delimitation", "trace".
    pub kind: String,
    pub busy: bool,
    pub calling: bool,
    pub linked_alignment: bool,
    pub species_view: bool,
    pub records_busy: bool,
    pub submission_busy: bool,
    pub trace_working: bool,
    pub review_mode: bool,
    pub unresolved: bool,
}

/// Why command `id` is unavailable in `context`, for its tooltip.
pub fn disabled_reason(id: &str, context: &Context) -> &'static str {
    let alignment = context.kind == "alignment";
    let trace = context.kind == "trace";
    if id.starts_with("metadata.") || id.starts_with("submission.") {
        return if !context.project {
            "Choose a project folder with File > Project to use project data"
        } else if ["metadata.import-headers", "metadata.template"].contains(&id) {
            "Open a FASTA file to use its records"
        } else if id == "submission.new" {
            "Open a marker alignment to start a submission"
        } else if context.submission_busy {
            "Wait for submission validation to finish"
        } else {
            "Start or open a GenBank submission in this project"
        };
    }
    if let Some(species) = id.strip_prefix("species.") {
        let analysis = ["gmyc-", "ptp-", "manual-", "asap-"]
            .iter()
            .any(|method| species.starts_with(method));
        return if species.starts_with("asap-") && !alignment && !context.linked_alignment {
            "Open an alignment or link one to the species workspace to run ASAP"
        } else if analysis && context.document && context.busy {
            "Wait for the current analysis to finish"
        } else if analysis {
            "Open an alignment, tree or species workspace to delimit species"
        } else if species.starts_with("combine-") {
            "Open a document, then choose species workspaces to combine"
        } else if !context.species_view {
            "Open a species delimitation workspace to use this command"
        } else if context.busy {
            "Wait for the workspace operation to finish"
        } else {
            match species {
                "roster-links" => {
                    "Choose a project folder and open its specimen metadata table to link records"
                }
                "threshold-cut" => {
                    "Attach an ultrametric display tree to enable threshold selection"
                }
                "display" => "A display tree is already attached; use the workspace's tree view",
                "layout" => "Choose at least one hypothesis in Columns to lay out its bars",
                "support-style" => "Attach a display tree with support values to style support",
                _ => "Attach a display tree to the species workspace to show it",
            }
        };
    }
    if id == "help.learn" {
        return "Manual pages are not written yet";
    }
    if id == "file.recover-work" {
        return "Wait for the current provenance operation to finish";
    }
    if !context.document {
        return "Open a document to use this command";
    }
    if id.starts_with("provenance.") && context.records_busy {
        return "Wait for the current provenance operation to finish";
    }
    if id == "trace.cancel-calls" {
        return "Compute alternative trace calls before cancelling them";
    }
    if trace && context.trace_working {
        return "Wait for trace processing to finish";
    }
    if context.busy || context.calling {
        return "Wait for the current operation to finish";
    }
    if trace && !context.review_mode {
        return "Open a Sanger review to edit or export its reviewed consensus";
    }
    if id == "trace.compute-calls" || id == "ribbon.combine-up-to-three-reads" {
        return "Open a single-read Sanger review to compute calls or draft additional reads";
    }
    if id == "ribbon.choose-alignment-context" {
        return "This review already has alignment context; open a standalone review to choose context";
    }
    if id.contains("edited-consensus") {
        return if context.unresolved {
            "Resolve or accept every remaining review issue before exporting the consensus"
        } else {
            "Choose alignment context for this review before adding its consensus"
        };
    }
    if id.contains("delete-columns-containing-only-gaps") {
        return "This alignment has no all-gap columns to delete";
    }
    if id.contains("which-support-values") {
        return "Use Support > Add to add tree support values before styling them";
    }
    if id == "ribbon.zoom-to-the-selection" {
        return "Open a sequence or alignment and select a region to zoom to it";
    }
    if id == "tree.rate-smoothing" {
        return "Save the tree to a file before smoothing its rates";
    }
    if id == "file.save-as" && trace {
        return "Open a Sanger review to save review decisions; a raw trace cannot be saved as a new document";
    }
    if !context.path {
        return "Save the document to a file before using this command";
    }
    "Open a compatible document to use this command"
}

/// What a BLAST search starts from: asking for the whole sequence while a
/// region is selected searches the selection; alignments keep their choice.
pub fn blast_source(alignment: bool, asked: &str, has_selection: bool) -> &str {
    if !alignment && asked == "whole" && has_selection {
        "selection"
    } else {
        asked
    }
}

/// The ribbon tabs for the active document's kind ("" with none open).
pub fn ribbon_tabs(kind: &str) -> Vec<&'static str> {
    let middle: &[&'static str] = match kind {
        "sequence" => &["Sequence"],
        "alignment" => &["Alignment", "Phylogeny", "Delimitation"],
        "tree" => &["Tree", "Delimitation"],
        "delimitation" => &["Delimitation"],
        "trace" => &["Trace"],
        _ => &[],
    };
    let mut tabs = vec!["File", "View"];
    tabs.extend_from_slice(middle);
    tabs.push("Export");
    tabs
}

/// The tab to show after the document kind changed: the chosen tab while
/// it still exists, else the document's first own tab, else File.
pub fn ribbon_tab(kind: &str, chosen: &str) -> &'static str {
    let tabs = ribbon_tabs(kind);
    if let Some(tab) = tabs.iter().find(|tab| **tab == chosen) {
        return tab;
    }
    if tabs.len() > 3 { tabs[2] } else { "File" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_listed_once_and_keys_have_a_scope() {
        let mut ids: Vec<_> = COMMANDS.iter().map(|command| command.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len());
        assert!(
            COMMANDS
                .iter()
                .all(|command| command.key.is_empty() || !command.scope.is_empty())
        );
        assert_eq!((key("file.save"), scope("file.save")), ("S", "File"));
        assert_eq!(
            (caption("file.save"), tip("file.save")),
            ("Save", "Save the current file or trace review")
        );
        assert_eq!(key("no.such.command"), "");
        assert_eq!(caption("species.gmyc-workspace"), "GMYC");
        assert_eq!(caption("tree.midpoint_root"), "Midpoint");
    }

    #[test]
    fn keys_do_not_collide_within_a_scope() {
        for a in COMMANDS {
            for b in COMMANDS {
                if a.id != b.id && a.scope == b.scope && !a.key.is_empty() && !b.key.is_empty() {
                    assert!(
                        !(a.key.starts_with(b.key) || b.key.starts_with(a.key)),
                        "{} and {} share key {} / {} in {}",
                        a.id,
                        b.id,
                        a.key,
                        b.key,
                        a.scope
                    );
                }
            }
        }
    }

    #[test]
    fn icons_come_from_the_list_then_the_id() {
        assert_eq!(icon("trace.compute-calls"), "trace-compute");
        assert_eq!(
            icon("ribbon.export-an-editable-svg-of-this-view"),
            "save-as"
        );
        assert_eq!(icon("ribbon.move-the-selected-rows-down"), "arrow-right");
        assert_eq!(icon(""), "info");
        assert_eq!(icon("view.find"), "info");
        assert_eq!(
            icon("ribbon.find-partial-terminal-primer-candidates-and-mask-them"),
            "trace-primer"
        );
    }

    #[test]
    fn unavailable_commands_explain_what_is_missing() {
        let none = Context::default();
        assert_eq!(
            disabled_reason("file.save", &none),
            "Open a document to use this command"
        );
        assert_eq!(
            disabled_reason("metadata.table", &none),
            "Choose a project folder with File > Project to use project data"
        );
        let busy = Context {
            document: true,
            busy: true,
            kind: "alignment".into(),
            ..Context::default()
        };
        assert_eq!(
            disabled_reason("file.save", &busy),
            "Wait for the current operation to finish"
        );
        assert_eq!(
            disabled_reason("species.gmyc-single", &busy),
            "Wait for the current analysis to finish"
        );
        let tree = Context {
            document: true,
            kind: "tree".into(),
            ..Context::default()
        };
        assert_eq!(
            disabled_reason("species.asap-run", &tree),
            "Open an alignment or link one to the species workspace to run ASAP"
        );
        assert_eq!(
            disabled_reason("tree.rate-smoothing", &tree),
            "Save the tree to a file before smoothing its rates"
        );
        let raw_trace = Context {
            document: true,
            kind: "trace".into(),
            ..Context::default()
        };
        assert_eq!(
            disabled_reason("file.export", &raw_trace),
            "Open a Sanger review to edit or export its reviewed consensus"
        );
    }

    #[test]
    fn blast_searches_a_selection_instead_of_the_whole_sequence() {
        assert_eq!(blast_source(false, "whole", true), "selection");
        assert_eq!(blast_source(false, "whole", false), "whole");
        assert_eq!(blast_source(false, "annotation", true), "annotation");
        assert_eq!(blast_source(true, "all", true), "all");
        assert_eq!(blast_source(true, "whole", true), "whole");
    }

    #[test]
    fn ribbon_tabs_follow_the_document_and_keep_a_chosen_tab() {
        assert_eq!(ribbon_tabs(""), ["File", "View", "Export"]);
        assert_eq!(
            ribbon_tabs("alignment"),
            [
                "File",
                "View",
                "Alignment",
                "Phylogeny",
                "Delimitation",
                "Export"
            ]
        );
        assert_eq!(ribbon_tab("alignment", "Sequence"), "Alignment");
        assert_eq!(ribbon_tab("tree", "Export"), "Export");
        assert_eq!(ribbon_tab("", "Tree"), "File");
        assert_eq!(ribbon_tab("tree", "Delimitation"), "Delimitation");
    }
}
