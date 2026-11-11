//! The desktop app's commands: each command's access key and the ribbon
//! scope it belongs to, its icon, why it is unavailable in a given state,
//! and which ribbon tabs a document kind shows. The ribbon, the shortcuts
//! dialog and key handling read this one list.

pub struct Command {
    pub id: &'static str,
    /// Access key within its scope, shown on the key hint overlay.
    pub key: &'static str,
    /// The ribbon tab or menu the key belongs to.
    pub scope: &'static str,
    /// An icon chosen for this command; others follow from its id.
    pub icon: &'static str,
}

pub static COMMANDS: &[Command] = &[
    Command {
        id: "alignment.blast",
        key: "B",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "analysis.distance.matrix",
        key: "M",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "analysis.distance.nj",
        key: "N",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "analysis.distance.upgma",
        key: "U",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "export.zenodo",
        key: "U",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "figure.export-png",
        key: "EX",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "file.new-sequence",
        key: "N",
        scope: "File",
        icon: "",
    },
    Command {
        id: "file.recover-work",
        key: "RE",
        scope: "File",
        icon: "",
    },
    Command {
        id: "file.save",
        key: "S",
        scope: "File",
        icon: "",
    },
    Command {
        id: "file.save-as",
        key: "A",
        scope: "File",
        icon: "",
    },
    Command {
        id: "metadata.import-headers",
        key: "H",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "metadata.import-sheet",
        key: "SS",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "metadata.table",
        key: "TT",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "metadata.template",
        key: "TE",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "project.open",
        key: "O",
        scope: "File",
        icon: "",
    },
    Command {
        id: "provenance.history",
        key: "P",
        scope: "File",
        icon: "",
    },
    Command {
        id: "provenance.methods",
        key: "M",
        scope: "File",
        icon: "",
    },
    Command {
        id: "ribbon.accept-this-base-and-go-to-the-next-issue",
        key: "AA",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.add-all-sequences-from-a-fasta-file",
        key: "F",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.add-support-values-from-another-analysis-of-the-same",
        key: "AD",
        scope: "Tree",
        icon: "support-add",
    },
    Command {
        id: "ribbon.add-the-edited-consensus-to-its-alignment",
        key: "AI",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.annotate-open-reading-frames",
        key: "O",
        scope: "Sequence",
        icon: "gene-orf",
    },
    Command {
        id: "ribbon.annotate-protein-coding-genes-with-prodigal",
        key: "PR",
        scope: "Sequence",
        icon: "gene-predict",
    },
    Command {
        id: "ribbon.annotate-the-selection",
        key: "A",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.associate-an-imported-tree-with-an-alignment-after-validating",
        key: "LI",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.choose-alignment-context",
        key: "AL",
        scope: "Trace",
        icon: "link",
    },
    Command {
        id: "ribbon.choose-the-consensus-range-to-keep-in-exports",
        key: "M",
        scope: "Trace",
        icon: "trace-mask",
    },
    Command {
        id: "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft",
        key: "SS",
        scope: "Trace",
        icon: "trace-cutoff",
    },
    Command {
        id: "ribbon.combine-up-to-three-reads",
        key: "D",
        scope: "Trace",
        icon: "trace-draft",
    },
    Command {
        id: "ribbon.copy-the-edited-consensus-as-fasta",
        key: "CO",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.copy-the-selected-annotation-s-sequence",
        key: "C",
        scope: "Sequence",
        icon: "clipboard",
    },
    Command {
        id: "ribbon.delete-columns-containing-only-gaps-across-all-rows-including",
        key: "DD",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.delete-the-selected-annotation",
        key: "D",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.delete-the-selected-rows",
        key: "DE",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.edit-the-selected-annotation",
        key: "EE",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.export-an-editable-svg-of-this-view",
        key: "EE",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "ribbon.export-the-edited-consensus-as-fasta",
        key: "SA",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.extract-aligned-rows-without-realigning-ctrl-click-to-select",
        key: "C",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.find-partial-terminal-primer-candidates-and-mask-them",
        key: "SE",
        scope: "Trace",
        icon: "trace-primer",
    },
    Command {
        id: "ribbon.fit",
        key: "F",
        scope: "View",
        icon: "",
    },
    Command {
        id: "ribbon.infer-a-tree-with-fasttree-standard-settings-gtr-for",
        key: "F",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "ribbon.insert-typing-inserts-bases-the-rest-of-the-row",
        key: "I",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.move-the-selected-rows-down",
        key: "DO",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.move-the-selected-rows-up",
        key: "U",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.name-the-selected-clade-with-a-bar-or-a",
        key: "AA",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.next-unresolved-issue",
        key: "N",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.order-every-clade-s-branches-by-size-largest-first",
        key: "L",
        scope: "Tree order menu",
        icon: "",
    },
    Command {
        id: "ribbon.order-every-clade-s-branches-by-size-smallest-first",
        key: "S",
        scope: "Tree order menu",
        icon: "",
    },
    Command {
        id: "ribbon.overwrite-typing-replaces-the-selected-bases",
        key: "O",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.previous-unresolved-issue",
        key: "P",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "ribbon.realign-the-sequences-with-mafft-default-settings-into-a",
        key: "M",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.rename-the-cursor-s-row",
        key: "R",
        scope: "Alignment",
        icon: "rename",
    },
    Command {
        id: "ribbon.reverse-the-order-of-the-branches-in-the-selected",
        key: "F",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.review-chromatograms-and-add-their-consensus",
        key: "C",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.root-the-tree-on-the-branch-above-the-selection",
        key: "R",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.save-the-selection-as-a-new-file",
        key: "EX",
        scope: "Sequence",
        icon: "selection",
    },
    Command {
        id: "ribbon.select-a-model-and-infer-a-maximum-likelihood-tree",
        key: "I",
        scope: "Phylogeny",
        icon: "",
    },
    Command {
        id: "ribbon.select-the-next-annotation",
        key: "N",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.select-the-previous-annotation",
        key: "PP",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "ribbon.trim-unreliable-columns-with-trimal-automated1-into-a-new",
        key: "TT",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.type-a-new-sequence-or-paste-fasta",
        key: "TY",
        scope: "Alignment",
        icon: "",
    },
    Command {
        id: "ribbon.which-support-values-show-circles-at-nodes-sizes",
        key: "ST",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "ribbon.zoom-in",
        key: "I",
        scope: "View",
        icon: "",
    },
    Command {
        id: "ribbon.zoom-out",
        key: "O",
        scope: "View",
        icon: "",
    },
    Command {
        id: "ribbon.zoom-to-the-selection",
        key: "S",
        scope: "View",
        icon: "",
    },
    Command {
        id: "sequence.blast",
        key: "B",
        scope: "Sequence",
        icon: "",
    },
    Command {
        id: "species.asap-workspace",
        key: "AA",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.columns",
        key: "CO",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.combine-project",
        key: "C",
        scope: "File",
        icon: "",
    },
    Command {
        id: "species.combine-workspace",
        key: "CC",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.details",
        key: "D",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.display",
        key: "AT",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.gmyc-workspace",
        key: "GG",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.large-on-top",
        key: "L",
        scope: "Species order menu",
        icon: "",
    },
    Command {
        id: "species.layout",
        key: "L",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.manual-workspace",
        key: "M",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.new",
        key: "N",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.order-branches",
        key: "O",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.ptp-workspace",
        key: "P",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.roster-links",
        key: "R",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.select-tool",
        key: "SE",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.small-on-top",
        key: "S",
        scope: "Species order menu",
        icon: "",
    },
    Command {
        id: "species.style-extra",
        key: "E",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.support-style",
        key: "SS",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.text-view",
        key: "TE",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.threshold-cut",
        key: "TH",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "species.tree-view",
        key: "TT",
        scope: "Delimitation",
        icon: "",
    },
    Command {
        id: "submission.new",
        key: "ST",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "submission.open",
        key: "O",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "submission.prepare",
        key: "P",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "submission.submitter",
        key: "SU",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "submission.validate",
        key: "V",
        scope: "Export",
        icon: "",
    },
    Command {
        id: "supportFont",
        key: "SS",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tipFont",
        key: "T",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "trace.cancel-calls",
        key: "CA",
        scope: "Trace",
        icon: "trace-cancel",
    },
    Command {
        id: "trace.compute-calls",
        key: "CC",
        scope: "Trace",
        icon: "trace-compute",
    },
    Command {
        id: "trace.keep-from",
        key: "KF",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "trace.keep-through",
        key: "KT",
        scope: "Trace",
        icon: "",
    },
    Command {
        id: "tree.metadata-labels",
        key: "LL",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.midpoint_root",
        key: "MM",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.minimum_variance_root",
        key: "MR",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.order-branches",
        key: "O",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.rate-smoothing",
        key: "P",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "tree.style-extra",
        key: "E",
        scope: "Tree",
        icon: "",
    },
    Command {
        id: "view.find",
        key: "D",
        scope: "View",
        icon: "",
    },
    Command {
        id: "view.text",
        key: "T",
        scope: "View",
        icon: "",
    },
    Command {
        id: "zoomPercentage",
        key: "QM",
        scope: "View",
        icon: "",
    },
];

fn find(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|command| command.id == id)
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
    fn every_command_is_listed_once_with_a_key_and_scope() {
        let mut ids: Vec<_> = COMMANDS.iter().map(|command| command.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len());
        assert!(
            COMMANDS
                .iter()
                .all(|command| !command.key.is_empty() && !command.scope.is_empty())
        );
        assert_eq!((key("file.save"), scope("file.save")), ("S", "File"));
        assert_eq!(key("no.such.command"), "");
    }

    #[test]
    fn keys_do_not_collide_within_a_scope() {
        for a in COMMANDS {
            for b in COMMANDS {
                if a.id != b.id && a.scope == b.scope {
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
