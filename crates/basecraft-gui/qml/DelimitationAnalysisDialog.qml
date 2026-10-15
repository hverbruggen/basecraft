import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami

AppFormDialog {
    id: dialog
    property string method: "gmyc"
    property string alignmentPath: ""
    property string treePath: ""
    property var pathResolver: null
    property int workers: 1
    signal requested(string method, string input, bool rooted, string settings, string displayMethod)
    readonly property bool viewSetup: method === "manual" || method === "view"
    readonly property bool alignmentInput: method === "asap" || (method === "gmyc" && source.currentIndex === 0)
    readonly property bool iqtreeDisplay: viewSetup ? treeChoice.currentIndex === 4 : method === "asap" && asapView.currentIndex === 3
    readonly property bool unsupportedDeduplication: !viewSetup && !alignmentInput && deduplicate.checked
    readonly property bool buildsTree: viewSetup && treeChoice.currentIndex > 1
    title: method === "view" ? "Species hypothesis display" : method === "manual" ? "Manual species delimitation" : method.toUpperCase() + " species delimitation"
    initialFocusItem: method === "gmyc" ? source : input
    standardButtons: Dialog.Ok | Dialog.Cancel
    function show(kind, alignment, tree) {
        method = kind
        alignmentPath = alignment
        treePath = tree
        source.currentIndex = alignment.length ? 0 : 1
        treeChoice.currentIndex = tree.length ? 1 : alignment.length ? 2 : 0
        input.text = alignmentInput ? alignment : tree
        rooted.checked = false
        deduplicate.checked = kind === "gmyc" && alignmentInput
        open()
    }
    function parameters() {
        const p = {workers: workers, seed: seed.value}
        if (alignmentInput || buildsTree) { p.model = distanceModel.currentText; p.min_overlap = overlap.value }
        if (!viewSetup) p.deduplicate = deduplicate.checked
        if (method === "gmyc") p.candidate_budget = budget.value
        if (iqtreeDisplay) p.iqtree_model = iqtreeModel.text.trim()
        if (method === "asap") p.replicates = replicates.value
        return p
    }
    function operation() {
        if (method === "gmyc") return "gmyc" + (alignmentInput ? "_alignment" : "") + (thresholds.currentIndex === 0 ? "_single" : "_multiple")
        if (method === "ptp") return rates.currentIndex === 0 ? "ptp" : "mptp"
        return method
    }
    readonly property bool validInput: viewSetup
        ? (treeChoice.currentIndex === 0 || (buildsTree ? alignmentPath.length > 0 : input.text.trim().length > 0)) && (!iqtreeDisplay || iqtreeModel.text.trim().length > 0)
        : !unsupportedDeduplication && input.text.trim().length > 0 && (alignmentInput || rooted.checked) && (!iqtreeDisplay || iqtreeModel.text.trim().length > 0)
    onOpened: {
        standardButton(Dialog.Ok).text = viewSetup ? "Open workspace" : "Run analysis"
        standardButton(Dialog.Ok).enabled = Qt.binding(() => validInput)
    }
    onAccepted: requested(operation(), buildsTree ? alignmentPath : input.text.trim(), rooted.checked,
        JSON.stringify(parameters()), viewSetup ? ["none", "existing", "upgma", "nj", "iqtree"][treeChoice.currentIndex]
        : method === "asap" ? ["none", "upgma", "nj", "iqtree"][asapView.currentIndex] : "existing")

    AppComboBox {
        id: source; objectName: "gmycSource"
        Kirigami.FormData.label: "Input"
        visible: dialog.method === "gmyc"
        model: ["Alignment → UPGMA time tree", "Existing rooted time tree"]
        onCurrentIndexChanged: input.text = currentIndex === 0 ? dialog.alignmentPath : dialog.treePath
    }
    AppComboBox {
        id: treeChoice; objectName: "manualTree"
        Kirigami.FormData.label: "View species on"
        visible: dialog.viewSetup
        model: ["Sequence groups (no tree)", "Existing tree", "Build UPGMA tree", "Build neighbor-joining tree", "Build IQ-TREE tree (substitutions)"]
        onCurrentIndexChanged: input.text = dialog.treePath
    }
    RowLayout {
        Kirigami.FormData.label: dialog.alignmentInput ? "Alignment" : "Tree"
        visible: !dialog.viewSetup || treeChoice.currentIndex === 1
        Layout.fillWidth: true
        TextField { id: input; objectName: "analysisInput"; Layout.fillWidth: true; Accessible.name: dialog.alignmentInput ? "Input alignment" : "Input tree"; selectByMouse: true }
        AppButton { text: "Browse…"; onClicked: picker.open() }
    }
    FileDialog {
        id: picker
        title: dialog.alignmentInput ? "Choose an aligned FASTA file" : "Choose a tree"
        onAccepted: if (dialog.pathResolver) input.text = dialog.pathResolver.local_file_path(selectedFile)
    }
    CheckBox {
        id: rooted; objectName: "rootedTree"
        Kirigami.FormData.label: "Root"
        text: "This tree is rooted"
        visible: !dialog.alignmentInput && !dialog.buildsTree && (!dialog.viewSetup || treeChoice.currentIndex === 1)
    }
    AppComboBox { id: thresholds; objectName: "gmycThresholds"; Kirigami.FormData.label: "GMYC model"; visible: dialog.method === "gmyc"; model: ["Single threshold", "Multiple thresholds"] }
    SpinBox { id: budget; Kirigami.FormData.label: "Candidate limit"; visible: dialog.method === "gmyc" && thresholds.currentIndex === 1; from: 1; to: 1000000; value: 1000; editable: true }
    CheckBox {
        id: deduplicate; objectName: "deduplicateInput"
        Kirigami.FormData.label: "Sampling"
        visible: !dialog.viewSetup
        text: "Deduplicate exact aligned DNA before analysis"
    }
    Label {
        Kirigami.FormData.isSection: true
        Layout.fillWidth: true; wrapMode: Text.Wrap
        text: dialog.viewSetup ? "Manual grouping and display preserve all specimens. Deduplication applies when running an analysis; Combine preserves saved hypotheses without rerunning them."
            : !dialog.alignmentInput ? "Tree-only deduplication is unavailable: it needs verified alignment evidence and a representative tree. Use alignment GMYC/ASAP, or prepare a representative tree and mapping in advance. Turn deduplication off to use this tree unchanged."
            : "Exact identity ignores letter case and treats U as T. Gaps and ambiguity codes match literally. One representative is fitted per identical sequence; all specimen IDs are restored in results. With deduplication off, all specimens enter the analysis. Display trees retain all specimens."
    }
    AppComboBox { id: rates; objectName: "ptpRates"; Kirigami.FormData.label: "PTP model"; visible: dialog.method === "ptp"; model: ["Single rate (PTP)", "Multiple rates (mPTP)"] }
    AppComboBox { id: distanceModel; Kirigami.FormData.label: "DNA distance model"; visible: dialog.alignmentInput || (dialog.buildsTree && !dialog.iqtreeDisplay); model: ["p", "jc69", "k2p"]; currentIndex: 2 }
    SpinBox { id: overlap; Kirigami.FormData.label: "Minimum shared sites"; visible: dialog.buildsTree || (dialog.method === "gmyc" && dialog.alignmentInput); from: 1; to: 100000000; value: 1; editable: true }
    TextField { id: iqtreeModel; objectName: "iqtreeModel"; Kirigami.FormData.label: "IQ-TREE model"; visible: dialog.iqtreeDisplay; text: "MFP"; Accessible.name: "IQ-TREE substitution model"; selectByMouse: true }
    SpinBox { id: seed; Kirigami.FormData.label: "Random seed"; visible: !dialog.viewSetup || dialog.iqtreeDisplay; from: 1; to: 2147483647; value: 1; editable: true }
    SpinBox { id: replicates; Kirigami.FormData.label: "ASAP simulations"; visible: dialog.method === "asap"; from: 100; to: 10000000; value: 10000; editable: true }
    AppComboBox { id: asapView; objectName: "asapView"; Kirigami.FormData.label: "View results on"; visible: dialog.method === "asap"; model: ["Sequence groups (no tree)", "UPGMA tree", "Neighbor-joining tree", "IQ-TREE tree (display only)"]; currentIndex: 1 }
    Label {
        Kirigami.FormData.isSection: true
        Layout.fillWidth: true; wrapMode: Text.Wrap
        text: dialog.method === "gmyc" ? "GMYC requires an ultrametric time tree. The alignment option builds an equal-rate UPGMA tree using your sampling choice. IQ-TREE substitution lengths are not a time tree; no dating or rooting is performed here."
            : dialog.method === "ptp" ? "PTP requires a rooted tree with branch lengths measured in substitutions. Infer an IQ-TREE tree from the alignment with the tree tools, explicitly root it, then select it here. No automatic rooting or dating is performed."
            : dialog.viewSetup ? "Click a node to select its sequences, then choose Group selection. A rooted ultrametric tree also supports a draggable threshold line. UPGMA provides this; neighbor joining and IQ-TREE support clade selection. IQ-TREE lengths are substitutions per site, not time."
            : "ASAP groups aligned DNA sequences. The display tree illustrates the results and does not change the species hypotheses. IQ-TREE lengths are substitutions per site, not time; model, seed and workers are retained."
    }
}
