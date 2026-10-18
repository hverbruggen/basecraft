import QtQuick
import QtQuick.Controls.Basic
import Basecraft

InstanceView {
    id: canvas
    property string accessibleTitle: "Document canvas"
    property string accessibleSummary: ""
    readonly property bool renderingAvailable: GraphicsInfo.api !== GraphicsInfo.Software && renderError.length === 0
    Accessible.role: Accessible.Pane
    Accessible.name: accessibleTitle
    Accessible.description: accessibleSummary + ". Press Ctrl+Alt+T for a text representation. Press F6 to move to another region."
    Accessible.focusable: true
    Accessible.focused: activeFocus
    // Set when keyboard region navigation (F6) focuses the canvas, so the
    // focus frame shows for keyboard users but not after a click or on open.
    property bool keyboardFocus: false
    onActiveFocusChanged: if (!activeFocus) keyboardFocus = false

    Rectangle {
        anchors.fill: parent
        z: 100
        color: "transparent"
        border.width: canvas.activeFocus && canvas.keyboardFocus ? 2 : 0
        border.color: palette.highlight
        SystemPalette { id: palette }
        Accessible.ignored: true
    }
    Rectangle {
        anchors.fill: parent
        z: 101
        visible: !canvas.renderingAvailable
        color: canvas.clearColor
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            onPressed: canvas.forceActiveFocus()
            onWheel: wheel => wheel.accepted = true
        }
        Label {
            anchors.centerIn: parent
            width: Math.max(0, Math.min(480, parent.width - 32))
            wrapMode: Text.Wrap
            text: canvas.renderError.length > 0 ? "The graphical canvas could not be drawn. " + canvas.renderError + " Use the text representation (Ctrl+Alt+T)." : "The graphical canvas is unavailable with Qt's software renderer. Use the text representation (Ctrl+Alt+T), or restart with a supported graphics backend."
        }
    }
}
