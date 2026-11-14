import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Name the support values read from another tree.
AppContentDialog {
    id: supportNameItem
    required property var app
    initialFocusItem: supportNameField
    property string path
    property var controller: null
    property var metadata: ({sets: []})
    title: "Name the support values"
    standardButtons: Dialog.Ok | Dialog.Cancel
    onOpened: { supportNameField.text = metadata.sets.length === 1 ? metadata.sets[0].name : ""; supportNameField.forceActiveFocus() }
    onAccepted: if (controller && !metadata.error) controller.add_support_from(path, supportNameField.text)
    body: ColumnLayout {
        spacing: 6
        Label { text: app.fileName(supportNameItem.path) }
        Label { text: supportNameItem.metadata.error || supportNameItem.metadata.sets.map(set => set.name).join(" / "); wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { id: supportNameFieldLabel; text: supportNameItem.metadata.sets.length > 1 ? "Optional prefix (individual names are retained)" : "Support values name" }
        TextField {
            Accessible.labelledBy: supportNameFieldLabel
            Accessible.name: supportNameFieldLabel.text
            id: supportNameField
            Layout.preferredWidth: 320
            placeholderText: "e.g. Bootstrap, PP, UFBoot; empty keeps the file's names"
            Keys.onReturnPressed: supportNameItem.accept()
            Keys.onEnterPressed: supportNameItem.accept()
        }
    }
}
