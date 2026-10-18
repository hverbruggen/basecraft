import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The FastTree settings that matter most, starting at Basecraft's standard
// run: GTR for DNA or LG for protein, CAT rates, SH-like support.
AppContentDialog {
    id: dialog
    property var target: null
    title: "FastTree"
    initialFocusItem: modelChoice
    standardButtons: Dialog.Ok | Dialog.Cancel
    readonly property var models: ["GTR for DNA, LG for protein (standard)", "JC (DNA)", "GTR (DNA)", "LG (protein)", "WAG (protein)", "JTT (protein)"]
    onOpened: standardButton(Dialog.Ok).text = "Build tree"
    onAccepted: if (target && !target.busy) target.build_fasttree(JSON.stringify(parameters()))
    function parameters() {
        const p = {gamma: gamma.checked, support: support.checked}
        if (modelChoice.currentIndex > 0) p.model = modelChoice.currentText.split(" ")[0]
        if (support.checked) p.resamples = resamples.value
        return p
    }
    body: ColumnLayout {
        spacing: 10
        Label { id: modelLabel; text: "Substitution model" }
        AppComboBox {
            id: modelChoice
            objectName: "fasttreeModel"
            Layout.fillWidth: true
            model: dialog.models
            Accessible.name: modelLabel.text
        }
        AppCheckBox {
            id: gamma
            objectName: "fasttreeGamma"
            text: "Gamma rate variation (rescales lengths; default: CAT categories)"
        }
        GridLayout {
            columns: 2
            columnSpacing: 12
            rowSpacing: 4
            AppCheckBox { id: support; objectName: "fasttreeSupport"; text: "SH-like local support"; checked: true }
            RowLayout {
                enabled: support.checked
                Label { id: resamplesLabel; text: "Resamples" }
                AppSpinBox { id: resamples; objectName: "fasttreeResamples"; from: 1; to: 100000; value: 1000; editable: true; Accessible.name: resamplesLabel.text }
            }
        }
        Label {
            Layout.preferredWidth: 1
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            opacity: 0.75
            text: "SH-like local support is a fast test of each branch, written on the tree as a value from 0 to 1. It is not a bootstrap proportion. Identical sequences are joined without a support value."
        }
    }
}
