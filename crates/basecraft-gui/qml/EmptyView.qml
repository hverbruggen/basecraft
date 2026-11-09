import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Shared guidance for an empty document, list or result. Callers retain command
// routing; informational states can omit an action when no direct action fits.
Control {
    id: root
    property string message
    property Action action
    property string actionText: action ? action.text : ""
    property string actionObjectName: "emptyViewAction"
    signal activated()
    padding: font.pixelSize
    implicitWidth: Math.min(48 * font.pixelSize, contentItem.implicitWidth + leftPadding + rightPadding)
    implicitHeight: contentItem.implicitHeight + topPadding + bottomPadding
    contentItem: ColumnLayout {
        spacing: root.font.pixelSize
        Label {
            Layout.fillWidth: true
            visible: text.length > 0
            text: root.message
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
        }
        AppButton {
            objectName: root.actionObjectName
            Layout.alignment: Qt.AlignHCenter
            visible: root.actionText.length > 0
            action: root.action
            text: root.actionText
            onClicked: root.activated()
        }
    }
}
