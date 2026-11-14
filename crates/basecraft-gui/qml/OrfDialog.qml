import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Find open reading frames in the open sequence or its selection.
AppContentDialog {
    id: orfDialogItem
    required property var app
    initialFocusItem: codons
    property bool inSelection: false
    property var source: null
    property string sourcePath: ""
    property var codes: []
    property int suggested: -1
    property string suggestedFrom: ""
    function search(selectionOnly) {
        if (!app.current || app.current.busy) return
        source = app.current
        sourcePath = source.file_path
        inSelection = selectionOnly
        codes = JSON.parse(source.genetic_codes())
        const code = source.genetic_code()
        suggested = codes.findIndex(c => c.id === code)
        suggestedFrom = source.genetic_code_source()
        orfCode.currentIndex = suggested
        open()
    }
    title: inSelection ? "Find open reading frames in the selection" : "Find open reading frames"
    standardButtons: Dialog.Ok | Dialog.Cancel
    onOpened: codons.forceActiveFocus()
    onAccepted: {
        app.orfMinCodons = codons.value
        if (orfCode.currentIndex < 0) return
        if (source && source.file_path === sourcePath && !source.busy) source.find_orfs(codons.value, codes[orfCode.currentIndex].id, orfDialogItem.inSelection)
    }
    body: ColumnLayout {
        spacing: 8
        RowLayout {
            Label { id: orfCodeLabel; text: "Genetic code" }
            AppComboBox {
                id: orfCode; objectName: "orfCode"; Accessible.labelledBy: orfCodeLabel; Accessible.name: orfCodeLabel.text
                Layout.fillWidth: true
                model: orfDialogItem.codes
                textRole: "name"
            }
            Label {
                text: orfCode.currentIndex === orfDialogItem.suggested ? "(" + orfDialogItem.suggestedFrom + ")" : ""
                opacity: 0.7
            }
        }
        RowLayout {
            Label { id: codonsLabel; text: "At least" }
            AppSpinBox {
                id: codons
                Accessible.labelledBy: codonsLabel
                Accessible.name: codonsLabel.text + " (codons, start to stop)"
                from: 10
                to: 10000
                stepSize: 10
                editable: true
                value: app.orfMinCodons
                Keys.onReturnPressed: orfDialogItem.accept()
                Keys.onEnterPressed: orfDialogItem.accept()
            }
            Label { text: "codons, start to stop" }
        }
        Label {
            text: "Searches both strands. Each ORF is added as a misc_feature with the note \"ORF\";\none Undo removes them all."
            opacity: 0.7
        }
    }
}
