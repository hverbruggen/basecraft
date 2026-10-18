import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Ask for one line of text, such as a new name. `ask` takes a callback that
// gets the trimmed text and returns an error to show, or nothing to close.
AppContentDialog {
    id: dialog
    objectName: "textPrompt"
    property var apply: null
    standardButtons: Dialog.Ok | Dialog.Cancel
    initialFocusItem: field
    function ask(heading, label, value, callback) {
        title = heading
        fieldLabel.text = label
        field.text = value
        apply = callback
        error.text = ""
        open()
    }
    onOpened: {
        standardButton(Dialog.Ok).enabled = Qt.binding(() => field.text.trim().length > 0)
        field.selectAll()
    }
    onAccepted: {
        const problem = apply ? apply(field.text.trim()) : ""
        if (problem) {
            error.text = problem
            open()
        }
    }
    body: ColumnLayout {
        spacing: 6
        Label { id: fieldLabel }
        TextField {
            id: field
            objectName: "promptValue"
            Layout.fillWidth: true
            Layout.preferredWidth: 320
            Accessible.labelledBy: fieldLabel
            Accessible.name: fieldLabel.text
            Keys.onReturnPressed: if (text.trim().length) dialog.accept()
            Keys.onEnterPressed: if (text.trim().length) dialog.accept()
        }
        AppErrorLabel { id: error; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true }
    }
}
