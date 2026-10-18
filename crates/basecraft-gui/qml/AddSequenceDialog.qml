import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppContentDialog {
    id: dialog
    initialFocusItem: inputKind === "fasta" ? sequenceInput : sequenceName
    required property var controller
    // Separate instances provide plain-text, FASTA and reviewed-consensus entry.
    property string inputKind: "text"
    property string reviewedSequence: ""
    property string validationMessage: ""
    signal inputError(string message)
    title: inputKind === "fasta" ? "Add sequences from FASTA" : inputKind === "reviewed" ? "Add reviewed consensus" : "Add sequence from text"
    implicitWidth: 600
    modal: true
    onOpened: {
        if (inputKind === "fasta") sequenceInput.forceActiveFocus()
        else { sequenceName.forceActiveFocus(); sequenceName.selectAll() }
    }
    function prepare(text, name = "", mafft = -1) {
        validationMessage = ""
        reviewedSequence = inputKind === "reviewed" ? text : ""
        sequenceInput.text = inputKind === "reviewed" ? "" : text
        sequenceName.text = name
        if (mafft >= 0) additionMode.currentIndex = mafft ? 1 : 0
        open()
    }
    function showChoices() { additionMode.popup.open() }
    function submit(keep) {
        validationMessage = ""
        if (inputKind !== "fasta" && !sequenceName.text.trim().length) {
            validationMessage = "Enter a sequence name."
            sequenceName.forceActiveFocus()
            return
        }
        const text = reviewedSequence.length ? reviewedSequence : sequenceInput.text
        if (inputKind === "text" && text.trim().startsWith(">")) {
            inputError("This dialog accepts plain sequence text. Use Add from FASTA for FASTA records.")
            return
        }
        if (inputKind === "fasta" && !text.trim().startsWith(">")) {
            inputError("FASTA records must begin with a >sequence_ID header.")
            return
        }
        const mafft = additionMode.currentIndex === 1
        controller.add_sequences(text, inputKind === "fasta" ? "" : sequenceName.text,
            mafft, mafft && checkOrientation.checked, mafft && keep)
    }
    body: ColumnLayout {
        spacing: 8
        Label { id: sequenceNameLabel; visible: dialog.inputKind !== "fasta"; text: "Sequence name" }
        TextField { id: sequenceName; Accessible.labelledBy: sequenceNameLabel; Accessible.name: sequenceNameLabel.text; objectName: "sequenceName"; visible: dialog.inputKind !== "fasta"; enabled: !dialog.controller.busy; Layout.fillWidth: true; placeholderText: "Specimen ID" }
        Label { id: sequenceInputLabel; visible: dialog.inputKind !== "reviewed"; text: dialog.inputKind === "fasta" ? "FASTA records (names come from their headers)" : "Sequence" }
        ScrollView {
            visible: dialog.inputKind !== "reviewed"
            Layout.fillWidth: true
            Layout.preferredHeight: 180
            TextArea { id: sequenceInput; Accessible.labelledBy: sequenceInputLabel; Accessible.name: sequenceInputLabel.text; objectName: "sequenceInput"; enabled: !dialog.controller.busy; wrapMode: TextEdit.Wrap; selectByMouse: true; placeholderText: dialog.inputKind === "fasta" ? ">specimen\nACGT…" : "ACGT…" }
        }
        Label { id: additionModeLabel; text: "Addition method" }
        AppComboBox { id: additionMode; objectName: "additionMode"; Accessible.labelledBy: additionModeLabel; Accessible.name: additionModeLabel.text; enabled: !dialog.controller.busy; model: ["Add unaligned", "Add with MAFFT"]; currentIndex: 1; Layout.fillWidth: true }
        AppCheckBox { visible: additionMode.currentIndex === 1; id: checkOrientation; text: "Check orientation of new sequences"; checked: true; enabled: !dialog.controller.busy && additionMode.currentIndex === 1 }
        AppCheckBox { visible: additionMode.currentIndex === 1; id: keepReference; text: "Forbid new gaps in the reference alignment"; checked: true; enabled: !dialog.controller.busy && additionMode.currentIndex === 1 }
        AppErrorLabel { text: dialog.validationMessage; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true; color: palette.highlight }
        RowLayout {
            Item { Layout.fillWidth: true }
            AppButton { text: "Cancel"; enabled: !dialog.controller.busy; onClicked: dialog.reject() }
            AppButton { text: dialog.controller.busy ? "Adding…" : "Add"; enabled: !dialog.controller.busy && (dialog.inputKind === "fasta" || sequenceName.text.trim().length > 0) && (dialog.inputKind === "reviewed" || sequenceInput.text.trim().length > 0); onClicked: dialog.submit(keepReference.checked) }
        }
    }
}
