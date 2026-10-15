import QtQuick
import QtQuick.Controls.Basic

ToolButton {
    id: button
    padding: 3
    background: Rectangle {
        color: Qt.alpha(button.palette.buttonText, button.down ? 0.14 : button.hovered ? 0.07 : 0)
        border.color: button.visualFocus ? button.palette.highlight : "transparent"
        radius: 3
    }
    property string iconName
    property string description
    Accessible.name: description
    display: AbstractButton.IconOnly
    icon.source: Qt.resolvedUrl("../icons/" + iconName + ".svg")
    icon.width: ["left", "right", "up", "down", "previous", "next"].includes(iconName) ? 10 : 16
    icon.height: ["left", "right", "up", "down", "previous", "next"].includes(iconName) ? 10 : 16
    icon.color: palette.buttonText
    ToolTip.text: description
    ToolTip.visible: hovered && description.length > 0
}
