import QtQuick
import QtQuick.Controls.Basic

ToolButton {
    id: button
    topPadding: 3
    bottomPadding: 3
    leftPadding: 6
    rightPadding: 6
    background: Rectangle {
        color: Qt.alpha(button.palette.buttonText, button.down || button.checked ? 0.14 : button.hovered ? 0.07 : 0)
        border.color: button.visualFocus ? button.palette.highlight : "transparent"
        radius: 3
    }
    property string tip
    property string keys
    property string badge
    property string iconName
    Accessible.name: text.length ? text : tip
    ToolTip.text: keys.length ? tip + "   " + keys : tip
    ToolTip.visible: (hovered || activeFocus) && tip.length > 0
    ToolTip.delay: 400
    // Keyboard access without taking editor focus on a mouse click.
    focusPolicy: Qt.TabFocus
    // Keep labels subordinate to the icon, and make the whole tile clickable.
    display: iconName.length ? AbstractButton.TextUnderIcon : AbstractButton.TextOnly
    font.pixelSize: Math.max(9, Math.round((button.Window.window && button.Window.window.font
        ? button.Window.window.font.pixelSize : 12) * 0.8))
    implicitWidth: Math.max(32, contentItem.implicitWidth + leftPadding + rightPadding)
    icon.source: iconName.length ? Qt.resolvedUrl("../icons/" + iconName + ".svg") : ""
    icon.width: ["left", "right", "up", "down", "previous", "next"].includes(iconName) ? 10 : 16
    icon.height: ["left", "right", "up", "down", "previous", "next"].includes(iconName) ? 10 : 16
    icon.color: palette.buttonText
    // Counts belong beside the symbol, leaving the action caption readable.
    contentItem: Item {
        implicitWidth: Math.max(symbols.implicitWidth, caption.implicitWidth)
        implicitHeight: (symbols.visible ? symbols.implicitHeight : 0) + (caption.visible ? caption.implicitHeight : 0) + (symbols.visible && caption.visible ? 2 : 0)
        Row {
            id: symbols
            anchors.horizontalCenter: parent.horizontalCenter
            visible: button.iconName.length > 0 || button.badge.length > 0
            spacing: 3
            AppIcon { visible: button.iconName.length > 0; name: button.iconName; size: button.icon.width; color: button.palette.buttonText }
            Text { visible: button.badge.length > 0; text: button.badge; font: button.font; color: button.palette.buttonText }
        }
        Text {
            id: caption
            anchors.horizontalCenter: parent.horizontalCenter
            y: symbols.visible ? symbols.implicitHeight + 2 : 0
            visible: text.length > 0
            text: button.text
            font: button.font
            color: button.palette.buttonText
        }
    }
}
