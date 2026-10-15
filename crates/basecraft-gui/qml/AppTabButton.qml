import QtQuick
import QtQuick.Controls.Basic

// Basic's inactive tabs assume a dark palette. Keep both states on the
// application's surface and size to text rather than a fixed 40-pixel minimum.
TabButton {
    id: tab
    topPadding: 3
    bottomPadding: 3
    leftPadding: 10
    rightPadding: 10
    contentItem: Text {
        text: tab.text
        font: tab.font
        color: tab.palette.windowText
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    background: Rectangle {
        color: Qt.tint(tab.palette.window, Qt.alpha(tab.palette.windowText,
            tab.checked ? 0 : tab.hovered ? 0.09 : 0.05))
        border.color: tab.visualFocus ? tab.palette.highlight : "transparent"
        Rectangle {
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            height: 2
            color: tab.palette.highlight
            visible: tab.checked
        }
    }
}
