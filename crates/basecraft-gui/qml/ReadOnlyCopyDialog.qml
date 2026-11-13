import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// D13: an edit refused because the file's folder cannot hold basecraft's
// recovery. Offer to save a copy elsewhere and edit that; Cancel or Escape
// leaves the file unedited. A refused trace review reopens on the copy.
AppContentDialog {
    id: dialog
    objectName: "readOnlyCopyDialog"
    property var owner: null
    property string dismissed: ""
    signal saveCopy(var refused)
    readonly property var refused: !owner ? null
        : owner.refused_edit !== undefined && owner.refused_edit.length ? JSON.parse(owner.refused_edit)
        : String(owner.message || "").startsWith("read_only_folder:") && owner.message !== dismissed ? {file: owner.file_path, review: true} : null
    onRefusedChanged: refused ? open() : close()
    title: "Read-only folder"
    standardButtons: Dialog.Cancel
    initialFocusItem: copy
    onRejected: {
        if (owner && owner.refused_edit !== undefined) owner.refused_edit = ""
        else if (owner) dismissed = owner.message
    }
    body: ColumnLayout {
        spacing: 6
        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: dialog.refused ? "The folder holding " + String(dialog.refused.file).split("/").pop() + " is read-only, so basecraft cannot keep recovery of edits there. Save a copy in a folder you can write to and edit the copy; the file here stays unedited." : ""
        }
        AppButton { id: copy; objectName: "saveEditableCopy"; text: "&Save a copy…"; onClicked: { const refused = dialog.refused; dialog.close(); dialog.saveCopy(refused) } }
    }
}
