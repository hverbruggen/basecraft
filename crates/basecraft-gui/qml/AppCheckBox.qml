import QtQuick
import QtQuick.Controls.Basic

CheckBox {
    id: control
    Accessible.name: text
    padding: 4
    spacing: 6
    FontMetrics { id: metrics; font: control.font }
    indicator: Rectangle {
        implicitWidth: Math.max(16, Math.ceil(metrics.height))
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
            size: Math.max(12, parent.width - 4)
            color: control.palette.text
            visible: control.checkState === Qt.Checked
        }
        Rectangle {
            anchors.centerIn: parent
            width: parent.width - 6; height: 2
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
