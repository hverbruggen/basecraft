import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Window

CheckBox {
    id: control
    Accessible.name: text
    padding: 4
    spacing: 6
    // The side set in Preferences, read from the main window.
    readonly property real boxSize: Window.window && Window.window.tickBoxSize ? Window.window.tickBoxSize : 14
    indicator: Rectangle {
        implicitWidth: control.boxSize
        implicitHeight: implicitWidth
        x: control.mirrored ? control.width - width - control.rightPadding : control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        color: control.down ? control.palette.light : control.palette.base
        border.width: control.visualFocus ? 2 : 1
        border.color: control.visualFocus ? control.palette.highlight : control.palette.mid
        opacity: control.enabled ? 1 : 0.45
        AppIcon {
            anchors.centerIn: parent
            name: "accept"
            size: Math.max(6, parent.width - 4)
            color: control.palette.text
            visible: control.checkState === Qt.Checked
        }
        Rectangle {
            anchors.centerIn: parent
            width: parent.width - 4; height: 2
            color: control.palette.text
            visible: control.checkState === Qt.PartiallyChecked
        }
    }
    contentItem: Text {
        text: control.text
        font: control.font
        color: control.palette.windowText
        opacity: control.enabled ? 1 : 0.45
        leftPadding: control.mirrored ? 0 : control.indicator.width + control.spacing
        rightPadding: control.mirrored ? control.indicator.width + control.spacing : 0
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.Wrap
    }
}
