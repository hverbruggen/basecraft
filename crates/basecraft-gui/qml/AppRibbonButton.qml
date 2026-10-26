import QtQuick
import QtQuick.Controls.Basic

ToolButton {
    id: button
    RibbonMetrics { id: metrics; control: button }
    readonly property real sizeFactor: metrics.factor
    property real symbolSize: ["left", "right", "up", "down", "previous", "next"].includes(iconName) ? 10 : 16
    topPadding: 3 * sizeFactor
    bottomPadding: 3 * sizeFactor
    leftPadding: 5 * sizeFactor
    rightPadding: 5 * sizeFactor
    background: Rectangle {
        color: Qt.alpha(button.palette.buttonText, button.down || button.checked ? 0.14 : button.hovered ? 0.07 : 0)
        border.color: button.visualFocus ? button.palette.highlight : "transparent"
        radius: 3
    }
    property string tip
    property string keys
    property string badge
    property string iconName
    // Keep accessible names and command identity intact while presenting
    // compact ribbon captions without trailing menu ellipses.
    readonly property string captionText: text.replace(/(?:\.\.\.|…)+\s*$/, "")
    readonly property real captionBaseline: contentItem.y + caption.y + caption.baselineOffset
    readonly property real symbolHeight: Math.max(metrics.symbolHeight, icon.height)
    Accessible.name: text.length ? text : tip
    ToolTip.text: keys.length ? tip + "   " + keys : tip
    ToolTip.visible: (hovered || activeFocus) && tip.length > 0
    ToolTip.delay: 400
    // Keyboard access without taking editor focus on a mouse click.
    focusPolicy: Qt.TabFocus
    // Keep labels subordinate to the icon, and make the whole tile clickable.
    display: iconName.length ? AbstractButton.TextUnderIcon : AbstractButton.TextOnly
    font.pixelSize: metrics.captionPixelSize
    implicitWidth: Math.max(32 * sizeFactor, contentItem.implicitWidth + leftPadding + rightPadding)
    icon.source: iconName.length ? Qt.resolvedUrl("../icons/" + iconName + ".svg") : ""
    icon.width: symbolSize * sizeFactor
    icon.height: symbolSize * sizeFactor
    icon.color: palette.buttonText
    // Counts belong beside the symbol, leaving the action caption readable.
    contentItem: Item {
        implicitWidth: Math.max(symbols.implicitWidth, caption.implicitWidth)
        implicitHeight: (symbols.visible ? symbols.implicitHeight : 0) + (caption.visible ? caption.implicitHeight : 0) + (symbols.visible && caption.visible ? 2 * button.sizeFactor : 0)
        Item {
            id: symbols
            anchors.horizontalCenter: parent.horizontalCenter
            visible: button.iconName.length > 0 || button.badge.length > 0
            implicitWidth: symbolRow.implicitWidth
            implicitHeight: button.symbolHeight
            Row {
                id: symbolRow
                anchors.centerIn: parent
                spacing: 3 * button.sizeFactor
                AppIcon { objectName: "ribbonSymbolIcon"; anchors.verticalCenter: parent.verticalCenter; visible: button.iconName.length > 0; name: button.iconName; size: button.symbolSize * button.sizeFactor; color: button.palette.buttonText }
                Text { anchors.verticalCenter: parent.verticalCenter; visible: button.badge.length > 0; text: button.badge; font: button.font; color: button.palette.buttonText }
            }
        }
        Text {
            id: caption
            anchors.horizontalCenter: parent.horizontalCenter
            y: symbols.visible ? symbols.implicitHeight + 2 * button.sizeFactor : 0
            visible: button.captionText.length > 0
            text: button.captionText
            font: button.font
            color: button.palette.buttonText
        }
    }
}
