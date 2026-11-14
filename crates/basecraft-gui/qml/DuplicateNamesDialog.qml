import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// D29: an opened file has rows, tips or records that share a name, so edits
// cannot tell them apart. Offer to number them (a.1, a.2), after the
// alphabet question if there is one. Cancel or Escape leaves the names.
AppContentDialog {
    id: dialog
    objectName: "duplicateNamesDialog"
    property var controller: null
    readonly property var offer: controller && controller.duplicate_names ? JSON.parse(controller.duplicate_names) : null
    readonly property bool asking: !!(controller && controller.alphabet_question)
    readonly property bool shown: offer !== null && !asking
    onShownChanged: shown ? open() : close()
    title: "Duplicate names"
    standardButtons: Dialog.Cancel
    initialFocusItem: standardButton(Dialog.Cancel)
    onRejected: if (controller) controller.duplicate_names = ""
    body: ColumnLayout {
        spacing: 6
        Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: dialog.offer ? dialog.offer.text : "" }
        AppButton { objectName: "numberDuplicates"; text: dialog.offer ? dialog.offer.action : ""; onClicked: dialog.controller.number_duplicates() }
    }
}
