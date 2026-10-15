import QtQuick

Rectangle {
    required property var control
    implicitWidth: 80
    implicitHeight: 30
    radius: 3
    color: Qt.tint(control.palette.button, Qt.alpha(control.palette.highlight,
        control.down === true || control.pressed === true || control.highlighted === true ? 0.18 : control.hovered === true ? 0.08 : 0))
    border.color: control.activeFocus ? control.palette.highlight : Qt.alpha(control.palette.buttonText, 0.28)
    opacity: control.enabled ? 1 : 0.45
}
