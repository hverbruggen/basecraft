import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Closing a tab with unsaved changes: save, discard or stay.
AppFormDialog {
    id: confirmCloseItem
    required property var app
    objectName: "closeDocumentDialog"
    property var target: null
    property string saveError: ""
    title: "Save changes?"
    standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
    AppErrorLabel {
        Accessible.role: confirmCloseItem.saveError.length ? Accessible.AlertMessage : Accessible.StaticText
        Kirigami.FormData.isSection: true
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        text: confirmCloseItem.saveError.length ? confirmCloseItem.saveError : "This file has unsaved changes."
    }
    onAccepted: {
        const owner = target
        if (app.documentOpens.target_index(owner.token, owner.path) < 0) return
        app.saveTarget(owner, "", function(success) {
            const row = app.documentOpens.target_index(owner.token, owner.path)
            if (row < 0) return
            if (!success) {
                confirmCloseItem.target = owner
                confirmCloseItem.saveError = owner.controller.message || "Save the review to a file first"
                confirmCloseItem.open()
                return
            }
            confirmCloseItem.saveError = ""
            app.removeDocument(row)
            Qt.callLater(app.refreshCurrent)
        })
    }
    onDiscarded: {
        const row = app.documentOpens.target_index(target.token, target.path)
        close()
        if (row < 0) return
        app.removeDocument(row)
        Qt.callLater(app.refreshCurrent)
    }
}
