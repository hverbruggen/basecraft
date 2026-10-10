import QtQuick
import QtQuick.Window
import Basecraft

Window {
    id: window
    width: 1200
    height: 760
    visible: true
    title: controller.title
    color: controller.background

    GenomeController {
        id: controller
    }

    component ToolButton: Rectangle {
        id: button
        property string text
        property bool active: false
        signal clicked
        width: label.implicitWidth + 20
        height: 26
        radius: 4
        color: active ? Qt.alpha(controller.accent, 0.18) : (area.containsMouse ? Qt.alpha(controller.foreground, 0.08) : "transparent")
        border.color: Qt.alpha(controller.foreground, 0.25)
        Text {
            id: label
            anchors.centerIn: parent
            text: button.text
            color: controller.foreground
            font.pixelSize: 13
        }
        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            onClicked: button.clicked()
        }
    }

    Row {
        id: toolbar
        anchors { top: parent.top; left: parent.left; margins: 8 }
        spacing: 6
        ToolButton { text: "Linear"; active: !controller.circular; onClicked: if (controller.circular) controller.toggle_mode() }
        ToolButton { text: "Circular"; active: controller.circular; onClicked: if (!controller.circular) controller.toggle_mode() }
        ToolButton { text: "Fit"; onClicked: controller.fit() }
        ToolButton { text: "Undo"; onClicked: controller.undo() }
        ToolButton { text: "Redo"; onClicked: controller.redo() }
        ToolButton { text: controller.dirty ? "Save •" : "Save"; onClicked: controller.save() }
    }

    Text {
        anchors { verticalCenter: toolbar.verticalCenter; right: parent.right; rightMargin: 12; left: toolbar.right; leftMargin: 16 }
        horizontalAlignment: Text.AlignRight
        elide: Text.ElideLeft
        text: controller.message
        color: controller.accent
        font.pixelSize: 13
    }

    InstanceView {
        id: view
        anchors { top: toolbar.bottom; topMargin: 8; left: parent.left; right: parent.right; bottom: statusBar.top }
        clearColor: controller.background
        instances: controller.frame
        focus: true
        onWidthChanged: controller.resize(width, height)
        onHeightChanged: controller.resize(width, height)

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton
            onPressed: (mouse) => { view.forceActiveFocus(); controller.press(mouse.x, mouse.y, mouse.modifiers) }
            onPositionChanged: (mouse) => controller.drag(mouse.x, mouse.y)
            onReleased: controller.release()
            onWheel: (wheel) => controller.wheel(wheel.x, wheel.angleDelta.y, wheel.angleDelta.x, wheel.modifiers)
        }

        Keys.onPressed: (event) => { event.accepted = controller.key(event.key, event.text, event.modifiers) }
    }

    Rectangle {
        id: statusBar
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: 26
        color: Qt.alpha(controller.foreground, 0.06)
        Text {
            anchors { verticalCenter: parent.verticalCenter; left: parent.left; leftMargin: 10; right: parent.right; rightMargin: 10 }
            elide: Text.ElideRight
            text: controller.status
            color: controller.foreground
            font.pixelSize: 13
        }
    }

    // Theme changes and typing bursts.
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

    // Scripted runs: --goto START,END  --circular 1  --type TEXT  --screenshot FILE
    // --type clicks the middle of the sequence track, then types TEXT.
    Timer {
        interval: 700
        running: arg("--screenshot") !== undefined
        onTriggered: {
            if (arg("--goto")) { const r = arg("--goto").split(","); controller.show_range(Number(r[0]), Number(r[1])) }
            if (arg("--circular") && !controller.circular) controller.toggle_mode()
            if (arg("--type")) {
                controller.press(view.width / 2, 45, 0)
                controller.release()
                for (const ch of arg("--type")) controller.key(ch.toUpperCase().charCodeAt(0), ch, 0)
            }
            shot.start()
        }
    }
    Timer {
        id: shot
        interval: 300
        onTriggered: {
            console.log(controller.memory_report(), "|", controller.status)
            console.log("saved", view.saveWindow(arg("--screenshot")))
            Qt.quit()
        }
    }
}
