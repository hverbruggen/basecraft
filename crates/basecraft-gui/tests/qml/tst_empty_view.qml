import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 700; height: 400
    property int opens: 0
    Action { id: openCommand; text: "Open"; onTriggered: opens++ }
    EmptyView {
        id: empty
        anchors.centerIn: parent
        width: parent.width
        message: "Open a document to begin."
        action: openCommand
    }
    TestCase {
        name: "EmptyView"
        when: windowShown
        function test_existing_action_keyboard_and_pointer_routes() {
            const button = findChild(empty, "emptyViewAction")
            button.forceActiveFocus()
            verify(button.activeFocus)
            keyClick(Qt.Key_Space)
            compare(opens, 1)
            mouseClick(button)
            compare(opens, 2)
            openCommand.enabled = false
            verify(!button.enabled)
            mouseClick(button)
            compare(opens, 2)
        }
    }
}
