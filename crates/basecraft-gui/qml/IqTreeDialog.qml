import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Settings for one IQ-TREE run: the model, or ModelFinder, and branch support.
AppContentDialog {
    id: dialog
    property var target: null
    title: "IQ-TREE"
    initialFocusItem: modelChoice
    standardButtons: Dialog.Ok | Dialog.Cancel
    readonly property var models: ["ModelFinder (best fit by BIC)",
        "JC (DNA)", "F81 (DNA)", "K2P (DNA)", "HKY (DNA)", "TN (DNA)", "TIM (DNA)", "TVM (DNA)", "GTR (DNA)",
        "LG (protein)", "WAG (protein)", "JTT (protein)", "Dayhoff (protein)", "mtREV (protein)", "cpREV (protein)", "Blosum62 (protein)"]
    readonly property bool modelFinder: modelChoice.currentIndex === 0
    onOpened: standardButton(Dialog.Ok).text = "Build tree"
    onAccepted: if (target && !target.busy) target.build_iqtree(JSON.stringify(parameters()))
    // The IQ-TREE model string, such as "MFP" or "GTR+I+G".
    function model() {
        if (modelFinder) return "MFP"
        return modelChoice.currentText.split(" ")[0] + (invariant.checked ? "+I" : "") + (gamma.checked ? "+G" : "")
    }
    function parameters() {
        const p = {model: model(), support: ["none", "ufboot", "bootstrap"][supportChoice.currentIndex], seed: seed.value}
        if (p.support !== "none") p.replicates = replicates.value
        return p
    }
    body: ColumnLayout {
        spacing: 10
        Label { id: modelLabel; text: "Substitution model" }
        AppComboBox {
            id: modelChoice
            objectName: "iqtreeModel"
            Layout.fillWidth: true
            model: dialog.models
            Accessible.name: modelLabel.text
        }
        RowLayout {
            enabled: !dialog.modelFinder
            AppCheckBox { id: gamma; objectName: "iqtreeGamma"; text: "Gamma rate variation (+G)"; checked: true }
            AppCheckBox { id: invariant; objectName: "iqtreeInvariant"; text: "Invariant sites (+I)" }
        }
        Label {
            Layout.preferredWidth: 1
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            opacity: 0.75
            text: dialog.modelFinder ? "ModelFinder tests models with and without rate variation and uses the best by BIC."
                : "Model: " + dialog.model()
        }
        AppFormRow {
            objectName: "iqtreeSupport"
            ColumnLayout {
                Layout.minimumWidth: 0
                Label { id: supportLabel; text: "Branch support"; Layout.fillWidth: true; wrapMode: Text.Wrap }
                AppComboBox {
                    id: supportChoice
                    objectName: "iqtreeSupportMethod"
                    model: ["None", "Ultrafast bootstrap (UFBoot)", "Standard bootstrap"]
                    Accessible.name: supportLabel.text
                    // UFBoot needs at least 1000 replicates; a standard bootstrap is slow, so start at 100.
                    onActivated: replicates.value = currentIndex === 1 ? Math.max(1000, replicates.value) : currentIndex === 2 ? 100 : replicates.value
                }
            }
            ColumnLayout {
                enabled: supportChoice.currentIndex > 0
                Layout.minimumWidth: 0
                Label { id: replicatesLabel; text: "Replicates"; Layout.fillWidth: true; wrapMode: Text.Wrap }
                AppSpinBox {
                    id: replicates
                    objectName: "iqtreeReplicates"
                    from: supportChoice.currentIndex === 1 ? 1000 : 1
                    to: 1000000
                    value: 1000
                    editable: true
                    Accessible.name: replicatesLabel.text
                }
            }
            ColumnLayout {
                Layout.minimumWidth: 0
                Label { id: seedLabel; text: "Seed"; Layout.fillWidth: true; wrapMode: Text.Wrap }
                AppSpinBox { id: seed; objectName: "iqtreeSeed"; from: 1; to: 2147483647; value: 1; editable: true; Accessible.name: seedLabel.text }
            }
        }
        Label {
            Layout.preferredWidth: 1
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: "Uses the allocated CPU threads. Support values are written on the branches of the tree."
        }
    }
}
