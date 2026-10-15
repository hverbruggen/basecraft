import QtQuick
import QtQuick.Controls.Basic
import Basecraft

// Shared tree rendering and navigation; the embedding workspace owns actions.
AppCanvas {
    id: view
        accessibleTitle: "Tree editor"
        accessibleSummary: treeController.status
    required property var controller
    readonly property var treeController: controller
    property bool annotationEnabled: true
    signal contextMenu(var info)
    signal toggleSidebar
    signal renameTip
    signal annotate
        clearColor: treeController.background
        fontFamily: treeController.mono_font
        instances: treeController.frame
        focus: true
        onWidthChanged: treeController.resize(width, height)
        onHeightChanged: treeController.resize(width, height)

        MouseArea {
            id: mouse
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            hoverEnabled: true
            property string hoverText: ""
            property real lastX: 0
            property real lastY: 0
            onPressed: (mouse) => {
                view.forceActiveFocus()
                hoverText = ""
                if (mouse.button === Qt.RightButton)
                    view.contextMenu(JSON.parse(treeController.context_press(mouse.x, mouse.y)))
                else
                    treeController.press_modified(mouse.x, mouse.y, mouse.modifiers)
            }
            onPositionChanged: (mouse) => {
                lastX = mouse.x; lastY = mouse.y
                hoverText = treeController.hover(mouse.x, mouse.y)
            }
            onExited: hoverText = ""
            onDoubleClicked: (mouse) => { if (annotationEnabled && treeController.has_selection) view.annotate() }
            onWheel: (wheel) => treeController.wheel(wheel.x, wheel.y, wheel.angleDelta.y, wheel.angleDelta.x, wheel.modifiers)
        }

        // What's under the pointer, following it.
        Rectangle {
            visible: mouse.hoverText.length > 0
            x: Math.min(mouse.lastX + 14, view.width - width - 4)
            y: mouse.lastY + 18
            width: tipLabel.implicitWidth + 16
            height: tipLabel.implicitHeight + 10
            radius: 4
            color: treeController.foreground
            opacity: 0.92
            Label {
                id: tipLabel
                anchors.centerIn: parent
                text: mouse.hoverText
                color: treeController.background
            }
        }

        ScrollBar {
            id: hbar
            orientation: Qt.Horizontal
            anchors { left: parent.left; right: vbar.left; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: treeController.hscroll_size < 0.999
            size: treeController.hscroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitHeight: 10
                radius: 5
                color: Qt.alpha(treeController.foreground, hbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(treeController.foreground, 0.06) }
            onPositionChanged: if (pressed) treeController.hscroll_to(position)
        }
        ScrollBar {
            id: vbar
            orientation: Qt.Vertical
            anchors { top: parent.top; right: parent.right; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: treeController.vscroll_size < 0.999
            width: visible ? implicitWidth : 0
            size: treeController.vscroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitWidth: 10
                radius: 5
                color: Qt.alpha(treeController.foreground, vbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(treeController.foreground, 0.06) }
            onPositionChanged: if (pressed) treeController.vscroll_to(position)
        }
        // Follow the controller except while the user drags a bar.
        Binding { target: vbar; property: "position"; value: treeController.vscroll_pos; when: !vbar.pressed; restoreMode: Binding.RestoreNone }
        Binding { target: hbar; property: "position"; value: treeController.hscroll_pos; when: !hbar.pressed; restoreMode: Binding.RestoreNone }

        PinchHandler {
            target: null
            property real last: 1
            onActiveChanged: last = 1
            onActiveScaleChanged: {
                treeController.wheel(centroid.position.x, centroid.position.y, Math.log(activeScale / last) / Math.log(1.001), 0, Qt.ControlModifier)
                last = activeScale
            }
        }

        Keys.onPressed: (event) => {
            if (!view.renderingAvailable) { event.accepted = false; return }
            const ctrl = event.modifiers & Qt.ControlModifier
            if (ctrl && event.key === Qt.Key_B) { view.toggleSidebar(); event.accepted = true; return }
            if (annotationEnabled && event.key === Qt.Key_F2) { view.renameTip(); event.accepted = true; return }
            if (annotationEnabled && (event.key === Qt.Key_Return || event.key === Qt.Key_Enter)) { view.annotate(); event.accepted = true; return }
            event.accepted = treeController.key(event.key, event.modifiers)
        }
    }
