import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Quitting with unsaved changes: save them all, throw them away, or stay.
AppFormDialog {
    id: quitDialogItem
    required property var app
    objectName: "quitDocumentsDialog"
    property var unsaved: []
    title: "Save changes before closing?"
    standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
    function ask(targets, error) {
        unsaved = targets
        quitError.text = error || ""
        open()
    }
    ColumnLayout {
        Kirigami.FormData.isSection: true
        Layout.fillWidth: true
        spacing: 6
        Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: quitDialogItem.unsaved.length === 1 ? "This file has unsaved changes:" : "These files have unsaved changes:" }
        Repeater {
            model: quitDialogItem.unsaved
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                required property var modelData
                text: "  " + app.fileName(modelData.path)
                font.bold: true
            }
        }
        AppErrorLabel { id: quitError; visible: text.length > 0; color: app.shell.accent; wrapMode: Text.Wrap }
    }
    onAccepted: app.quitStep(JSON.parse(app.documentOpens.save_all(JSON.stringify(unsaved))))
    onDiscarded: { close(); app.quitStep(JSON.parse(app.documentOpens.discard_all(JSON.stringify(unsaved)))) }
}
