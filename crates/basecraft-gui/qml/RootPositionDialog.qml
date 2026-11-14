import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Where the root sits along its branch: a share of the branch on each side,
// halfway after rerooting.
AppContentDialog {
    id: rootDialogItem
    required property var app
    initialFocusItem: rootShare
    property var info: ({ side: "", other: "", length: 0, share: 0.5 })
    title: "Root position"
    standardButtons: Dialog.Ok | Dialog.Cancel
    function ask() {
        if (!app.treeOpen) return
        const i = JSON.parse(app.current.root_json())
        if (!i) {
            app.current.message_kind = "error"
            app.current.message = "The root needs two branches with lengths"
            return
        }
        info = i
        rootShare.value = i.share * 100
        open()
    }
    function length(share) { return (info.length * share).toPrecision(3) }
    onAccepted: app.current.move_root(rootShare.value / 100)
    body: ColumnLayout {
        spacing: 8
        Label { id: rootShareLabel; text: "Share of the root branch on each side"; opacity: 0.7 }
        GridLayout {
            columns: width < 600 ? 1 : 3
            Layout.fillWidth: true
            Label { text: rootDialogItem.info.side; elide: Text.ElideMiddle; Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true; horizontalAlignment: Text.AlignRight }
            Slider { id: rootShare; Accessible.labelledBy: rootShareLabel; Accessible.name: rootShareLabel.text + " (%)"; from: 0; to: 100; stepSize: 0.5; Layout.preferredWidth: 220; Layout.minimumWidth: 0; Layout.fillWidth: true }
            Label { text: rootDialogItem.info.other; elide: Text.ElideMiddle; Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true }
        }
        GridLayout {
            columns: width < 600 ? 1 : 3
            Layout.fillWidth: true
            Label {
                text: rootShare.value.toFixed(1) + " % · " + rootDialogItem.length(rootShare.value / 100)
                Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true
                horizontalAlignment: Text.AlignRight
            }
            AppButton { text: "Halfway"; flat: true; Layout.preferredWidth: 220; Layout.minimumWidth: 0; Layout.fillWidth: true; onClicked: rootShare.value = 50 }
            Label { text: (100 - rootShare.value).toFixed(1) + " % · " + rootDialogItem.length(1 - rootShare.value / 100); Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true }
        }
    }
}
