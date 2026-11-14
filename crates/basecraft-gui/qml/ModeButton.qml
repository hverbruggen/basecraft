import QtQuick

// A ribbon button that shows whether its mode is on.
RibbonButton {
    id: modeButton
    property bool active
    background: Rectangle {
        radius: 3
        color: !modeButton.appWindow ? "transparent" : modeButton.active ? Qt.alpha(modeButton.appWindow.palette.highlight, 0.18)
            : modeButton.hovered ? Qt.alpha(modeButton.appWindow.palette.windowText, 0.05) : "transparent"
        border.color: modeButton.appWindow && (modeButton.active || modeButton.visualFocus) ? modeButton.appWindow.palette.highlight : "transparent"
    }
}
