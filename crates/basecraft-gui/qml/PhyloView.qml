import QtQuick
import QtQuick.Controls
import Basecraft

// One open tree: its controller, the GPU view, and pointer and key
// handling. Window-level actions (menus, dialogs, the sidebar) are asked
// for through signals, as in the other editors.
Item {
    id: doc
    required property string path
    required property int index
    property alias controller: ctl
    property alias view: view
    readonly property bool dirty: ctl.dirty
    readonly property string kind: "tree"

    signal contextMenu(var info)
    signal toggleSidebar
    signal renameTip
    signal annotate
    signal openFile(string path)

    TreeController { id: ctl }
    Connections {
        target: ctl
        function onFileCreated(path) { doc.openFile(path) }
    }

    // Lines snap to whole screen pixels.
    readonly property real pixelRatio: view.Window.window ? view.Window.window.devicePixelRatio : 1
    onPixelRatioChanged: ctl.set_pixel_ratio(pixelRatio)

    // Open once the view has its real size.
    Component.onCompleted: Qt.callLater(() => {
        ctl.set_pixel_ratio(doc.pixelRatio)
        ctl.resize(view.width, view.height)
        ctl.open_file(doc.path)
    })

    InstanceView {
        id: view
        anchors.fill: parent
        clearColor: ctl.background
        fontFamily: ctl.mono_font
        instances: ctl.frame
        focus: true
        onWidthChanged: ctl.resize(width, height)
        onHeightChanged: ctl.resize(width, height)

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
                    doc.contextMenu(JSON.parse(ctl.context_press(mouse.x, mouse.y)))
                else
                    ctl.press_modified(mouse.x, mouse.y, mouse.modifiers)
            }
            onPositionChanged: (mouse) => {
                lastX = mouse.x; lastY = mouse.y
                hoverText = ctl.hover(mouse.x, mouse.y)
            }
            onExited: hoverText = ""
            onDoubleClicked: (mouse) => { if (ctl.has_selection) doc.annotate() }
            onWheel: (wheel) => ctl.wheel(wheel.x, wheel.y, wheel.angleDelta.y, wheel.angleDelta.x, wheel.modifiers)
        }

        // What's under the pointer, following it.
        Rectangle {
            visible: mouse.hoverText.length > 0
            x: Math.min(mouse.lastX + 14, view.width - width - 4)
            y: mouse.lastY + 18
            width: tipLabel.implicitWidth + 16
            height: tipLabel.implicitHeight + 10
            radius: 4
            color: ctl.foreground
            opacity: 0.92
            Label {
                id: tipLabel
                anchors.centerIn: parent
                text: mouse.hoverText
                color: ctl.background
            }
        }

        ScrollBar {
            id: hbar
            orientation: Qt.Horizontal
            anchors { left: parent.left; right: vbar.left; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: ctl.hscroll_size < 0.999
            size: ctl.hscroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitHeight: 10
                radius: 5
                color: Qt.alpha(ctl.foreground, hbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(ctl.foreground, 0.06) }
            onPositionChanged: if (pressed) ctl.hscroll_to(position)
        }
        ScrollBar {
            id: vbar
            orientation: Qt.Vertical
            anchors { top: parent.top; right: parent.right; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: ctl.vscroll_size < 0.999
            width: visible ? implicitWidth : 0
            size: ctl.vscroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitWidth: 10
                radius: 5
                color: Qt.alpha(ctl.foreground, vbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(ctl.foreground, 0.06) }
            onPositionChanged: if (pressed) ctl.vscroll_to(position)
        }
        // Follow the controller except while the user drags a bar.
        Binding { target: vbar; property: "position"; value: ctl.vscroll_pos; when: !vbar.pressed; restoreMode: Binding.RestoreNone }
        Binding { target: hbar; property: "position"; value: ctl.hscroll_pos; when: !hbar.pressed; restoreMode: Binding.RestoreNone }

        PinchHandler {
            target: null
            property real last: 1
            onActiveChanged: last = 1
            onActiveScaleChanged: {
                ctl.wheel(centroid.position.x, centroid.position.y, Math.log(activeScale / last) / Math.log(1.001), 0, Qt.ControlModifier)
                last = activeScale
            }
        }

        Keys.onPressed: (event) => {
            const ctrl = event.modifiers & Qt.ControlModifier
            if (ctrl && event.key === Qt.Key_B) { doc.toggleSidebar(); event.accepted = true; return }
            if (event.key === Qt.Key_F2) { doc.renameTip(); event.accepted = true; return }
            if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) { doc.annotate(); event.accepted = true; return }
            event.accepted = ctl.key(event.key, event.modifiers)
        }
    }
}
