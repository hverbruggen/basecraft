import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import Basecraft

ApplicationWindow {
    id: window
    width: 1200
    height: 760
    visible: true
    title: controller.title
    color: controller.background

    palette.window: controller.background
    palette.base: Qt.lighter(controller.background, 1.04)
    palette.button: Qt.darker(controller.background, 1.05)
    palette.text: controller.foreground
    palette.windowText: controller.foreground
    palette.buttonText: controller.foreground
    palette.highlight: controller.accent
    palette.highlightedText: controller.background

    GenomeController {
        id: controller
    }

    // Clipboard access goes through a hidden text edit.
    TextEdit {
        id: clipboard
        visible: false
        function copy(text) { clipboard.text = text; clipboard.selectAll(); clipboard.copy() }
    }

    component Tool: ToolButton {
        font.pixelSize: 13
        focusPolicy: Qt.NoFocus
    }

    header: ToolBar {
        background: Rectangle { color: Qt.alpha(controller.foreground, 0.04) }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 6
            spacing: 2
            Tool { text: "Wrapped"; checkable: true; checked: controller.mode === "wrapped"; onClicked: controller.switch_view("wrapped") }
            Tool { text: "Linear"; checkable: true; checked: controller.mode === "linear"; onClicked: controller.switch_view("linear") }
            Tool { text: "Circular"; checkable: true; checked: controller.mode === "circular"; onClicked: controller.switch_view("circular") }
            ToolSeparator {}
            Tool { text: "−"; ToolTip.text: "Zoom out (−)"; ToolTip.visible: hovered; onClicked: controller.zoom_out() }
            Tool { text: "+"; ToolTip.text: "Zoom in (+, double-click)"; ToolTip.visible: hovered; onClicked: controller.zoom_in() }
            Tool { text: "Fit"; ToolTip.text: "Whole sequence (Ctrl+0)"; ToolTip.visible: hovered; onClicked: controller.fit() }
            Tool { text: "Zoom to selection"; onClicked: controller.zoom_to_selection() }
            Label {
                text: controller.zoom_text
                leftPadding: 8
                rightPadding: 4
                font.pixelSize: 12
                opacity: 0.75
            }
            ToolSeparator {}
            Tool { text: "Add feature…"; ToolTip.text: "Annotate the selection (Ctrl+N)"; ToolTip.visible: hovered; onClicked: editor.openNew() }
            Tool { text: "Edit feature…"; enabled: true; ToolTip.text: "Edit the selected feature (Enter)"; ToolTip.visible: hovered; onClicked: editor.openSelected() }
            ToolSeparator {}
            Tool { text: "Undo"; onClicked: controller.undo() }
            Tool { text: "Redo"; onClicked: controller.redo() }
            Tool { text: controller.dirty ? "Save •" : "Save"; onClicked: controller.save() }
            Label {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignRight
                rightPadding: 10
                elide: Text.ElideLeft
                text: controller.message
                color: controller.accent
            }
        }
    }

    footer: Rectangle {
        height: 26
        color: Qt.alpha(controller.foreground, 0.06)
        Label {
            anchors { verticalCenter: parent.verticalCenter; left: parent.left; leftMargin: 10; right: parent.right; rightMargin: 10 }
            elide: Text.ElideRight
            text: controller.status
            font.pixelSize: 13
        }
    }

    InstanceView {
        id: view
        anchors.fill: parent
        anchors.topMargin: 6
        clearColor: controller.background
        instances: controller.frame
        focus: true
        onWidthChanged: controller.resize(width, height)
        onHeightChanged: controller.resize(width, height)

        MouseArea {
            id: mouse
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
            hoverEnabled: true
            property string hoverText: ""
            onPressed: (mouse) => {
                view.forceActiveFocus()
                hoverText = ""
                if (mouse.button === Qt.RightButton) {
                    menu.info = JSON.parse(controller.context_press(mouse.x, mouse.y))
                    menu.popup()
                } else if (mouse.button === Qt.MiddleButton) {
                    controller.pan_press(mouse.x, mouse.y)
                } else {
                    controller.press(mouse.x, mouse.y, mouse.modifiers)
                }
            }
            property real lastX: 0
            property real lastY: 0
            onPositionChanged: (mouse) => {
                lastX = mouse.x; lastY = mouse.y
                if (mouse.buttons & (Qt.LeftButton | Qt.MiddleButton)) {
                    controller.drag(mouse.x, mouse.y)
                    edgeTimer.running = (mouse.buttons & Qt.LeftButton) && (mouse.x < 24 || mouse.x > view.width - 24)
                } else {
                    hoverText = controller.hover(mouse.x, mouse.y)
                }
            }
            onExited: hoverText = ""
            onReleased: { edgeTimer.running = false; controller.release() }
            // Keeps scrolling while a selection drag sits at or past an edge.
            Timer {
                id: edgeTimer
                interval: 16
                repeat: true
                onTriggered: running = controller.edge_scroll(parent.lastX, parent.lastY) && parent.pressed
            }
            onDoubleClicked: (mouse) => {
                const info = JSON.parse(controller.context_press(mouse.x, mouse.y))
                if (info.feature >= 0) editor.openFeature(info.feature)
                else controller.double_click(mouse.x, mouse.y)
            }
            onWheel: (wheel) => controller.wheel(wheel.x, wheel.angleDelta.y, wheel.angleDelta.x, wheel.modifiers)
        }

        // Feature tooltip, following the pointer.
        Rectangle {
            visible: mouse.hoverText.length > 0
            x: Math.min(mouse.lastX + 14, view.width - width - 4)
            y: mouse.lastY + 18
            width: tipLabel.implicitWidth + 16
            height: tipLabel.implicitHeight + 10
            radius: 4
            color: controller.foreground
            opacity: 0.92
            Label {
                id: tipLabel
                anchors.centerIn: parent
                text: mouse.hoverText
                color: controller.background
                font.pixelSize: 13
            }
        }

        ScrollBar {
            id: hbar
            orientation: Qt.Horizontal
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: controller.mode === "linear" && controller.scroll_size < 0.999
            size: controller.scroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitHeight: 10
                radius: 5
                color: Qt.alpha(controller.foreground, hbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(controller.foreground, 0.06) }
            onPositionChanged: if (pressed) controller.scroll_to(position)
        }
        ScrollBar {
            id: vbar
            orientation: Qt.Vertical
            anchors { top: parent.top; right: parent.right; bottom: parent.bottom }
            policy: ScrollBar.AlwaysOn
            visible: controller.mode === "wrapped" && controller.scroll_size < 0.999
            size: controller.scroll_size
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitWidth: 10
                radius: 5
                color: Qt.alpha(controller.foreground, vbar.pressed ? 0.65 : 0.4)
            }
            background: Rectangle { color: Qt.alpha(controller.foreground, 0.06) }
            onPositionChanged: if (pressed) controller.scroll_to(position)
        }
        Binding {
            target: vbar
            property: "position"
            value: controller.scroll_pos
            when: !vbar.pressed
            restoreMode: Binding.RestoreNone
        }
        // Follow the controller except while the user drags the bar, so the
        // binding survives interaction.
        Binding {
            target: hbar
            property: "position"
            value: controller.scroll_pos
            when: !hbar.pressed
            restoreMode: Binding.RestoreNone
        }

        PinchHandler {
            id: pinch
            target: null
            property real last: 1
            onActiveChanged: last = 1
            onActiveScaleChanged: {
                controller.zoom_by(centroid.position.x, activeScale / last)
                last = activeScale
            }
        }

        Keys.onPressed: (event) => {
            if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && controller.selected_feature() >= 0) {
                editor.openSelected(); event.accepted = true; return
            }
            if (event.key === Qt.Key_N && (event.modifiers & Qt.ControlModifier)) {
                editor.openNew(); event.accepted = true; return
            }
            if (event.key === Qt.Key_C && (event.modifiers & Qt.ControlModifier)) {
                const t = controller.selection_text()
                if (t.length > 0) clipboard.copy(t)
                event.accepted = true; return
            }
            event.accepted = controller.key(event.key, event.text, event.modifiers)
        }
    }

    Menu {
        id: menu
        property var info: ({ feature: -1, label: "", selection: false, length: 0 })
        MenuItem { text: "Add feature from selection…"; enabled: menu.info.selection; onTriggered: editor.openNew() }
        MenuItem { text: "Edit “" + menu.info.label + "”…"; visible: menu.info.feature >= 0; height: visible ? implicitHeight : 0; onTriggered: editor.openFeature(menu.info.feature) }
        MenuItem { text: "Delete “" + menu.info.label + "”"; visible: menu.info.feature >= 0; height: visible ? implicitHeight : 0; onTriggered: controller.delete_feature(menu.info.feature) }
        MenuItem { text: "Select its residues"; visible: menu.info.feature >= 0; height: visible ? implicitHeight : 0; onTriggered: controller.select_feature_range(menu.info.feature) }
        MenuItem { text: "Zoom to feature"; visible: menu.info.feature >= 0; height: visible ? implicitHeight : 0; onTriggered: controller.zoom_to_feature(menu.info.feature) }
        MenuSeparator {}
        MenuItem { text: "Copy sequence (" + menu.info.length + " bp)"; enabled: menu.info.selection; onTriggered: clipboard.copy(controller.selection_text()) }
        MenuItem { text: "Reverse-complement selection"; enabled: menu.info.selection; onTriggered: controller.reverse_complement_selection() }
        MenuItem { text: "Zoom to selection"; enabled: menu.info.selection; onTriggered: controller.zoom_to_selection() }
        MenuSeparator {}
        MenuItem { text: "Zoom in"; onTriggered: controller.zoom_in() }
        MenuItem { text: "Zoom out"; onTriggered: controller.zoom_out() }
        MenuItem { text: "Whole sequence"; onTriggered: controller.fit() }
    }

    FeatureEditor {
        id: editor
        controller: controller
        onClosed: view.forceActiveFocus()
    }

    Timer { interval: 1500; running: true; repeat: true; onTriggered: { controller.check_theme(); controller.flush() } }

    function arg(name) {
        const args = Qt.application.arguments
        const i = args.indexOf(name)
        return i >= 0 ? args[i + 1] : undefined
    }

    Component.onCompleted: {
        const args = Qt.application.arguments
        for (let i = 1; i < args.length; i++) {
            if (args[i].startsWith("--")) { i++; continue }
            controller.open_file(args[i])
            break
        }
        controller.resize(view.width, view.height)
    }

    // Scripted runs: --goto START,END  --mode wrapped|linear|circular  --circular 1  --type TEXT
    // --select START,END  --menu X,Y  --edit FEATURE  --screenshot FILE
    Timer {
        interval: 700
        running: arg("--screenshot") !== undefined
        onTriggered: {
            if (arg("--goto")) { const r = arg("--goto").split(","); controller.show_range(Number(r[0]), Number(r[1])) }
            if (arg("--mode")) controller.switch_view(arg("--mode"))
            if (arg("--circular") && !controller.circular) controller.switch_view("circular")
            if (arg("--type")) {
                controller.press(view.width / 2, 45, 0)
                controller.release()
                for (const ch of arg("--type")) controller.key(ch.toUpperCase().charCodeAt(0), ch, 0)
            }
            if (arg("--select")) {
                // X0,X1[,Y]: a left-button drag at height Y (default: the sequence).
                const r = arg("--select").split(",")
                const y = r.length > 2 ? Number(r[2]) : 45
                controller.press(Number(r[0]), y, 0); controller.drag(Number(r[1]), y); controller.release()
            }
            if (arg("--add")) {
                // --add LOCATION: add a misc_feature there, e.g. "<100..>400".
                const err = controller.save_feature(-1, JSON.stringify({ kind: "misc_feature", location: arg("--add"), qualifiers: [{ key: "label", value: "partial test" }] }))
                if (err.length > 0) console.log("add failed:", err)
            }
            if (arg("--hover")) {
                const h = arg("--hover").split(",")
                console.log("hover:", controller.hover(Number(h[0]), Number(h[1])))
            }
            if (arg("--menu")) {
                const p = arg("--menu").split(",")
                menu.info = JSON.parse(controller.context_press(Number(p[0]), Number(p[1])))
                menu.popup(Number(p[0]), Number(p[1]) + view.y)
            }
            if (arg("--edit")) {
                if (arg("--edit") === "new") editor.openNew(); else editor.openFeature(Number(arg("--edit")))
            }
            shot.start()
        }
    }
    Timer {
        id: shot
        interval: 400
        onTriggered: {
            console.log(controller.memory_report(), "|", controller.status)
            console.log("saved", view.saveWindow(arg("--screenshot")))
            Qt.quit()
        }
    }
}
