import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// D29: an opened file has rows, tips or records that share a name, so edits
// cannot tell them apart. Offer to number them (a.1, a.2). Cancel or Escape
// leaves the names as they are until the file is opened again.
AppContentDialog {
    id: dialog
    objectName: "duplicateNamesDialog"
    property var controller: null
    readonly property string offer: controller && controller.duplicate_names !== undefined ? controller.duplicate_names : ""
    onOfferChanged: offer.length ? open() : close()
    title: "Duplicate names"
    standardButtons: Dialog.Cancel
    initialFocusItem: number
    onRejected: if (controller) controller.duplicate_names = ""
    body: ColumnLayout {
        spacing: 6
        Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: dialog.offer }
        AppButton { id: number; objectName: "numberDuplicates"; text: "&Number them"; onClicked: dialog.controller.number_duplicates() }
    }
}
