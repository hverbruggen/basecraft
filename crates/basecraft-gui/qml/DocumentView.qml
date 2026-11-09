import QtQuick
import QtQuick.Controls.Basic
import Basecraft

// One open file: its controller, the GPU view, and pointer and key
// handling. Window-level actions (menus, dialogs, the sidebar) are asked
// for through signals.
FocusScope {
    id: doc
    property bool overviewEnabled: true
    property var notificationSink: null
    Connections {
        target: ctl
        function onMessageChanged() {
            if (notificationSink && ctl.message.length)
                notificationSink.notification(ctl.file_path, ctl.message, ctl.message_kind)
        }
    }

    required property string path
    required property int index
    property alias controller: ctl
    property alias view: view
    readonly property bool dirty: ctl.dirty
    readonly property string kind: "sequence"
    property bool showGc: true
    // Layout a newly opened file starts in: "wrapped", "linear" or "circular".
    property string defaultLayout: "wrapped"
    onShowGcChanged: ctl.set_show_gc(showGc)

    signal contextMenu(var info)
    signal editFeature(int id)
    signal newAnnotation
    signal editSelected
    signal copyText(string text)
    signal toggleSidebar

    GenomeController { id: ctl }

    // Open once the view has its real size, so the fitted zoom is right.
    Component.onCompleted: Qt.callLater(() => {
        ctl.resize(view.width, view.height)
        ctl.set_show_gc(doc.showGc)
        ctl.open_file(doc.path)
        if (doc.defaultLayout !== "wrapped") {
            ctl.switch_view(doc.defaultLayout)
            ctl.fit()
        }
    })

    AppCanvas {
        id: view
        accessibleTitle: "Sequence editor"
        accessibleSummary: ctl.status
        anchors.fill: parent
        anchors.bottomMargin: overview.visible ? overview.height : 0
        clearColor: ctl.background
        fontFamily: ctl.mono_font
        instances: ctl.frame
        focus: true
        onWidthChanged: ctl.resize(width, height)
        onHeightChanged: ctl.resize(width, height)

        MouseArea {
            id: mouse
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
            hoverEnabled: true
            property string hoverText: ""
            property real lastX: 0
            property real lastY: 0
            onPressed: (mouse) => {
                view.forceActiveFocus()
                hoverText = ""
                if (mouse.button === Qt.RightButton)
                    doc.contextMenu(JSON.parse(ctl.context_press(mouse.x, mouse.y)))
                else if (mouse.button === Qt.MiddleButton)
                    ctl.pan_press(mouse.x, mouse.y)
                else
                    ctl.press(mouse.x, mouse.y, mouse.modifiers)
            }
            onPositionChanged: (mouse) => {
                lastX = mouse.x; lastY = mouse.y
                if (mouse.buttons & (Qt.LeftButton | Qt.MiddleButton)) {
                    ctl.drag(mouse.x, mouse.y)
                    edgeTimer.running = (mouse.buttons & Qt.LeftButton)
                        && (mouse.x < 24 || mouse.x > view.width - 24 || mouse.y < 24 || mouse.y > view.height - 24)
                } else {
                    hoverText = ctl.hover(mouse.x, mouse.y)
                }
            }
            onExited: hoverText = ""
            onReleased: { edgeTimer.running = false; ctl.release() }
            // Keeps scrolling while a selection drag sits at or past an edge.
            Timer {
                id: edgeTimer
                interval: 16
                repeat: true
                onTriggered: running = ctl.edge_scroll(parent.lastX, parent.lastY) && parent.pressed
            }
            onDoubleClicked: (mouse) => {
                const info = JSON.parse(ctl.context_press(mouse.x, mouse.y))
                if (info.feature >= 0) doc.editFeature(info.feature)
                else ctl.double_click(mouse.x, mouse.y)
            }
            onWheel: (wheel) => ctl.wheel(wheel.x, wheel.y, wheel.angleDelta.y, wheel.angleDelta.x, wheel.modifiers)
        }

        // Annotation tooltip, following the pointer.
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
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: ctl.mode === "linear" && ctl.scroll_size < 0.999
            size: ctl.scroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitHeight: 10
                radius: 5
                color: Qt.alpha(ctl.foreground, hbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(ctl.foreground, 0.06) }
            onPositionChanged: if (pressed) ctl.scroll_to(position)
        }
        ScrollBar {
            id: vbar
            orientation: Qt.Vertical
            anchors { top: parent.top; right: parent.right; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: ctl.mode === "wrapped" && ctl.scroll_size < 0.999
            size: ctl.scroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitWidth: 10
                radius: 5
                color: Qt.alpha(ctl.foreground, vbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(ctl.foreground, 0.06) }
            onPositionChanged: if (pressed) ctl.scroll_to(position)
        }
        // Follow the controller except while the user drags a bar, so the
        // binding survives interaction.
        Binding { target: vbar; property: "position"; value: ctl.scroll_pos; when: !vbar.pressed; restoreMode: Binding.RestoreNone }
        Binding { target: hbar; property: "position"; value: ctl.scroll_pos; when: !hbar.pressed; restoreMode: Binding.RestoreNone }

        PinchHandler {
            target: null
            property real last: 1
            onActiveChanged: last = 1
            onActiveScaleChanged: {
                ctl.zoom_by(centroid.position.x, centroid.position.y, activeScale / last)
                last = activeScale
            }
        }

        Keys.onPressed: (event) => {
            if (!view.renderingAvailable) { event.accepted = false; return }
            const ctrl = event.modifiers & Qt.ControlModifier
            if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && ctl.selected_feature() >= 0) {
                doc.editSelected(); event.accepted = true; return
            }
            if (ctrl && event.key === Qt.Key_B) { doc.toggleSidebar(); event.accepted = true; return }
            if (ctrl && event.key === Qt.Key_E) { ctl.extract_selection(); event.accepted = true; return }
            if (ctrl && event.key === Qt.Key_N) { doc.newAnnotation(); event.accepted = true; return }
            if (ctrl && event.key === Qt.Key_C) {
                const t = ctl.selection_text()
                if (t.length > 0) doc.copyText(t)
                event.accepted = true; return
            }
            event.accepted = ctl.key(event.key, event.text, event.modifiers)
        }
    }
    DocumentOverview {
        id: overview
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        shown: doc.overviewEnabled
        circular: ctl.mode === "circular"
        miniature: ctl.overview_content
        foreground: ctl.foreground; backgroundColor: ctl.background; accent: ctl.accent
        documentItem: view
        viewportX: ctl.overview_pos; viewportWidth: ctl.overview_size
        onPanRequested: (x, y) => ctl.overview_scroll_to(x)
    }
}
