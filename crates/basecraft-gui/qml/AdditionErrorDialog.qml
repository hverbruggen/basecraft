import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppContentDialog {
    id: dialog
    initialFocusItem: closeButton
    property string message: ""
    signal copyMessage(string text)
    title: "Cannot add sequences"
    closePolicy: Popup.CloseOnEscape
    onOpened: Qt.callLater(() => closeButton.forceActiveFocus())
    function show(message) { dialog.message = message; open() }
    Connections {
        target: dialog
        function onOpened() { errorText.Accessible.announce(dialog.message) }
    }
    body: ColumnLayout {
        spacing: 10
        ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(280, errorText.implicitHeight)
            TextArea {
                id: errorText
                Accessible.name: dialog.title
                Accessible.description: dialog.message
                onTextChanged: if (dialog.opened && text.length) Accessible.announce(text)
                objectName: "additionErrorText"
                text: dialog.message
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
            }
        }
        RowLayout {
            Item { Layout.fillWidth: true }
            AppButton { objectName: "additionErrorCopy"; text: "Copy message"; onClicked: dialog.copyMessage(dialog.message) }
            AppButton {
                id: closeButton
                objectName: "additionErrorClose"
                text: "Close"
                onClicked: dialog.close()
                Keys.onReturnPressed: dialog.close()
                Keys.onEnterPressed: dialog.close()
            }
        }
    }
}
