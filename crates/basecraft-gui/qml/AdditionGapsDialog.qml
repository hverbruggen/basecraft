import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppContentDialog {
    id: dialog
    property int count: 0
    property var additionDialog: null
    initialFocusItem: abortAddition
    title: "New bases require reference gaps"
    // Escape and Abort addition cancel the same pending addition. Neither may
    // retry with different gap settings; only Allow new gaps can do that.
    onRejected: if (additionDialog) additionDialog.reject()
    body: ColumnLayout {
        Label {
            text: dialog.count + " bases in the new sequences cannot fit without adding gaps to the reference. Allow new gaps to retain every base, or abort the addition."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        RowLayout {
            AppButton { id: abortAddition; objectName: "abortAddition"; text: "Abort addition"; onClicked: dialog.reject() }
            AppButton {
                text: "Allow new gaps"
                onClicked: { dialog.close(); if (dialog.additionDialog) dialog.additionDialog.submit(false) }
            }
        }
    }
}
