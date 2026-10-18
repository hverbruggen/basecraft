import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Dialogs

AppContentDialog {
    id: dialog
    property string method: "gmyc"
    property string alignmentPath: ""
    property string treePath: ""
    property var pathResolver: null
    property int workers: 1
    // Method → null when its external engine is installed, else the reason.
    property var engineProblems: ({})
    readonly property string engineProblem: method === "ptp" ? (engineProblems[rates.currentIndex === 0 ? "ptp" : "mptp"] || "") : ""
    signal requested(string method, string input, bool rooted, string settings, string displayMethod)
    readonly property bool viewSetup: method === "manual" || method === "view"
    readonly property bool alignmentInput: method === "asap" || (method === "gmyc" && source.currentIndex === 0)
    // PTP can build its own tree: IQ-TREE from the alignment, then rooted.
    readonly property bool ptpFromAlignment: method === "ptp" && ptpSource.currentIndex === 1
    readonly property bool iqtreeDisplay: viewSetup ? treeChoice.currentIndex === 4 : (method === "asap" && asapView.currentIndex === 3) || ptpFromAlignment
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
        ptpSource.currentIndex = alignment.length && !tree.length ? 1 : 0
        treeChoice.currentIndex = tree.length ? 1 : alignment.length ? 2 : 0
        input.text = alignmentInput || ptpFromAlignment ? alignment : tree
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
        if (ptpFromAlignment) p.rooting = ["midpoint", "mrttv"][rooting.currentIndex]
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
        : !engineProblem.length && !unsupportedDeduplication && input.text.trim().length > 0 && (alignmentInput || ptpFromAlignment || rooted.checked) && (!iqtreeDisplay || iqtreeModel.text.trim().length > 0)
    onOpened: {
        standardButton(Dialog.Ok).text = viewSetup ? "Open workspace" : "Run analysis"
        standardButton(Dialog.Ok).enabled = Qt.binding(() => validInput)
    }
    onAccepted: requested(operation(), buildsTree ? alignmentPath : input.text.trim(), rooted.checked || ptpFromAlignment,
        JSON.stringify(parameters()), viewSetup ? ["none", "existing", "upgma", "nj", "iqtree"][treeChoice.currentIndex]
        : method === "asap" ? ["none", "upgma", "nj", "iqtree"][asapView.currentIndex] : ptpFromAlignment ? "iqtree" : "existing")

    body: ColumnLayout {
        spacing: 10
        AppErrorLabel {
            objectName: "engineProblem"
            Layout.preferredWidth: 1
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            visible: text.length > 0
            text: dialog.engineProblem
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: dialog.method === "gmyc"
            Label {
                id: sourceLabel
                text: "Input"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppComboBox {
                id: source
                objectName: "gmycSource"
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                model: ["Alignment → UPGMA time tree", "Existing rooted time tree"]
                Accessible.labelledBy: sourceLabel
                Accessible.name: sourceLabel.text
                onCurrentIndexChanged: input.text = currentIndex === 0 ? dialog.alignmentPath : dialog.treePath
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: dialog.method === "ptp"
            Label {
                id: ptpSourceLabel
                text: "Input"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppComboBox {
                id: ptpSource
                objectName: "ptpSource"
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                model: ["Existing rooted tree", "Alignment → IQ-TREE tree, then rooted"]
                enabled: dialog.alignmentPath.length > 0
                Accessible.labelledBy: ptpSourceLabel
                Accessible.name: ptpSourceLabel.text
                onCurrentIndexChanged: input.text = currentIndex === 1 ? dialog.alignmentPath : dialog.treePath
            }
            Label {
                id: rootingLabel
                visible: dialog.ptpFromAlignment
                text: "Root the tree by"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppComboBox {
                id: rooting
                objectName: "ptpRooting"
                visible: dialog.ptpFromAlignment
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                model: ["Midpoint", "Minimum variance (MRTTV)"]
                Accessible.labelledBy: rootingLabel
                Accessible.name: rootingLabel.text
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: dialog.viewSetup
            Label {
                id: treeLabel
                text: "View species on"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppComboBox {
                id: treeChoice
                objectName: "manualTree"
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                model: ["Text view (no tree)", "Existing tree", "Build UPGMA tree", "Build neighbor-joining tree", "Build IQ-TREE tree (substitutions)"]
                Accessible.labelledBy: treeLabel
                Accessible.name: treeLabel.text
                onCurrentIndexChanged: input.text = dialog.treePath
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: !dialog.viewSetup || treeChoice.currentIndex === 1
            Label {
                id: inputLabel
                text: dialog.alignmentInput || dialog.ptpFromAlignment ? "Alignment" : "Tree"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            RowLayout {
                Layout.fillWidth: true
                TextField {
                    id: input
                    objectName: "analysisInput"
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Accessible.labelledBy: inputLabel
                    Accessible.name: dialog.alignmentInput ? "Input alignment" : "Input tree"
                    selectByMouse: true
                }
                AppButton {
                    text: "Browse…"
                    onClicked: picker.open()
                }
            }
        }
        FileDialog {
            id: picker
            title: dialog.alignmentInput || dialog.ptpFromAlignment ? "Choose an aligned FASTA file" : "Choose a tree"
            onAccepted: if (dialog.pathResolver)
                input.text = dialog.pathResolver.local_file_path(selectedFile)
        }
        AppCheckBox {
            id: rooted
            objectName: "rootedTree"
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            text: "This tree is rooted"
            visible: !dialog.alignmentInput && !dialog.ptpFromAlignment && !dialog.buildsTree && (!dialog.viewSetup || treeChoice.currentIndex === 1)
        }
        AppFormRow {
            objectName: "samplingSettings"
            visible: !dialog.viewSetup
            ColumnLayout {
                visible: dialog.method === "gmyc"
                Layout.minimumWidth: 0
                Label {
                    id: thresholdsLabel
                    text: "GMYC model"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                AppComboBox {
                    id: thresholds
                    objectName: "gmycThresholds"
                    Accessible.labelledBy: thresholdsLabel
                    Accessible.name: thresholdsLabel.text
                    model: ["Single threshold", "Multiple thresholds"]
                }
            }
            ColumnLayout {
                Layout.minimumWidth: 0
                Label {
                    id: samplingLabel
                    text: "Sampling"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                AppCheckBox {
                    id: deduplicate
                    objectName: "deduplicateInput"
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Accessible.labelledBy: samplingLabel
                    text: "Deduplicate exact aligned DNA"
                }
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: dialog.method === "gmyc" && thresholds.currentIndex === 1
            Label {
                id: budgetLabel
                text: "Candidate limit"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppSpinBox {
                id: budget
                objectName: "candidateLimit"
                from: 1
                to: 1000000
                value: 1000
                editable: true
                Accessible.name: budgetLabel.text
                Accessible.labelledBy: budgetLabel
            }
        }
        Label {
            Layout.preferredWidth: 1
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: dialog.viewSetup ? "Manual grouping and display preserve all specimens. Deduplication applies when running an analysis; Combine preserves saved hypotheses without rerunning them." : !dialog.alignmentInput ? "Tree-only deduplication is unavailable: it needs verified alignment evidence and a representative tree. Use alignment GMYC/ASAP, or prepare a representative tree and mapping in advance. Turn deduplication off to use this tree unchanged." : "Exact identity ignores letter case and treats U as T. Gaps and ambiguity codes match literally. One representative is fitted per identical sequence; all specimen IDs are restored in results. With deduplication off, all specimens enter the analysis. Display trees retain all specimens."
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: dialog.method === "ptp"
            Label {
                id: ratesLabel
                text: "PTP model"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppComboBox {
                id: rates
                objectName: "ptpRates"
                model: ["Single rate (PTP)", "Multiple rates (mPTP)"]
                Accessible.name: ratesLabel.text
                Accessible.labelledBy: ratesLabel
                Layout.fillWidth: true
                Layout.minimumWidth: 0
            }
        }
        AppFormRow {
            objectName: "distanceSettings"
            visible: dialog.alignmentInput || dialog.buildsTree
            ColumnLayout {
                Layout.minimumWidth: 0
                visible: dialog.alignmentInput || (dialog.buildsTree && !dialog.iqtreeDisplay)
                Label {
                    id: distanceLabel
                    text: "DNA distance model"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                AppComboBox {
                    id: distanceModel
                    objectName: "distanceModel"
                    Accessible.labelledBy: distanceLabel
                    Accessible.name: distanceLabel.text
                    model: ["p", "jc69", "k2p"]
                    currentIndex: 2
                }
            }
            ColumnLayout {
                Layout.minimumWidth: 0
                visible: dialog.buildsTree || (dialog.method === "gmyc" && dialog.alignmentInput)
                Label {
                    id: overlapLabel
                    text: "Minimum shared sites"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                AppSpinBox {
                    id: overlap
                    objectName: "distanceOverlap"
                    Accessible.labelledBy: overlapLabel
                    Accessible.name: overlapLabel.text
                    from: 1
                    to: 100000000
                    value: 1
                    editable: true
                }
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: dialog.iqtreeDisplay
            Label {
                id: iqtreeLabel
                text: "IQ-TREE model"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            TextField {
                id: iqtreeModel
                objectName: "iqtreeModel"
                text: "MFP"
                Accessible.name: "IQ-TREE substitution model"
                selectByMouse: true
                Accessible.labelledBy: iqtreeLabel
                Layout.fillWidth: true
                Layout.minimumWidth: 0
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: !dialog.viewSetup || dialog.iqtreeDisplay
            Label {
                id: randomSeedLabel
                text: "Random seed"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppSpinBox {
                id: seed
                objectName: "randomSeed"
                from: 1
                to: 2147483647
                value: 1
                editable: true
                Accessible.name: randomSeedLabel.text
                Accessible.labelledBy: randomSeedLabel
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: dialog.method === "asap"
            Label {
                id: simulationsLabel
                text: "ASAP simulations per node (maximum)"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppSpinBox {
                id: replicates
                objectName: "asapSimulations"
                from: 100
                to: 10000000
                value: 100000
                editable: true
                Accessible.name: simulationsLabel.text
                Accessible.labelledBy: simulationsLabel
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: dialog.method === "asap"
            Label {
                id: viewLabel
                text: "View results on"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            AppComboBox {
                id: asapView
                objectName: "asapView"
                model: ["Text view (no tree)", "UPGMA tree", "Neighbor-joining tree", "IQ-TREE tree (display only)"]
                currentIndex: 1
                Accessible.name: viewLabel.text
                Accessible.labelledBy: viewLabel
                Layout.fillWidth: true
                Layout.minimumWidth: 0
            }
        }
        Label {
            Layout.preferredWidth: 1
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: dialog.method === "gmyc" ? "GMYC requires an ultrametric time tree. The alignment option builds an equal-rate UPGMA tree using your sampling choice. IQ-TREE substitution lengths are not a time tree; no dating or rooting is performed here." : dialog.method === "ptp" ? (dialog.ptpFromAlignment ? "PTP requires a rooted tree with branch lengths measured in substitutions. IQ-TREE infers a maximum-likelihood tree from all sequences, which is rooted by the method chosen and saved as its own tree file; PTP then runs on that tree, which also becomes the display tree. An outgroup rooting needs an existing tree." : "PTP requires a rooted tree with branch lengths measured in substitutions, such as an IQ-TREE tree you have rooted. No rooting or dating is done here.") : dialog.viewSetup ? "Click a node to select its sequences, then choose Group selection. A rooted ultrametric tree also supports a draggable threshold line. UPGMA provides this; neighbor joining and IQ-TREE support clade selection. IQ-TREE lengths are substitutions per site, not time." : "ASAP groups aligned DNA sequences. The display tree illustrates the results and does not change the species hypotheses. IQ-TREE lengths are substitutions per site, not time; model, seed and workers are retained."
        }
    }
}
