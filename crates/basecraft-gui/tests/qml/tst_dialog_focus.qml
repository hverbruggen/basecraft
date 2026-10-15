import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"

Item {
    width: 640; height: 480
    TextField { id: editor; text: "Document" }
    AppDialog {
        id: dialog
        title: "Edit name"
        standardButtons: Dialog.Ok | Dialog.Cancel
        property alias field: name
        contentItem: ColumnLayout {
            Label { text: "Name" }
            TextField { enabled: false; text: "Disabled field" }
            TextField { visible: false; text: "Hidden field" }
            TextField { id: name; text: "Name" }
            Button { text: "Other control" }
        }
    }
    AppDialog {
        id: confirmation
        title: "Discard changes?"
        standardButtons: Dialog.Discard | Dialog.Cancel
        initialFocusItem: standardButton(Dialog.Cancel)
    }
    TestCase {
        name: "DialogFocus"
        when: windowShown
        function init() { editor.forceActiveFocus(); tryCompare(editor, "activeFocus", true) }
        function cleanup() { confirmation.close(); dialog.close(); editor.enabled = true }
        function test_initial_and_restore() {
            dialog.open()
            tryCompare(dialog.field, "activeFocus", true)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            tryCompare(editor, "activeFocus", true)
        }
        function test_modal_tab_cycle() {
            dialog.open()
            tryCompare(dialog.field, "activeFocus", true)
            for (let i = 0; i < 12; ++i) {
                keyClick(Qt.Key_Tab)
                verify(!editor.activeFocus, "Tab must remain in the modal dialog")
            }
            keyClick(Qt.Key_Backtab)
            verify(!editor.activeFocus)
        }
        function test_nested_safe_focus() {
            dialog.open()
            tryCompare(dialog.field, "activeFocus", true)
            confirmation.open()
            tryCompare(confirmation.standardButton(Dialog.Cancel), "activeFocus", true)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog.field, "activeFocus", true)
            dialog.close()
            tryCompare(editor, "activeFocus", true)
        }
        function test_accept_restores() {
            dialog.open()
            tryCompare(dialog.field, "activeFocus", true)
            dialog.accept()
            tryCompare(editor, "activeFocus", true)
        }
        function test_accept_opens_next_dialog() {
            const next = () => confirmation.open()
            dialog.accepted.connect(next)
            dialog.open()
            tryCompare(dialog.field, "activeFocus", true)
            dialog.accept()
            tryCompare(confirmation.standardButton(Dialog.Cancel), "activeFocus", true)
            dialog.accepted.disconnect(next)
        }
        function test_disabled_invoker_is_not_focused() {
            dialog.open()
            tryCompare(dialog.field, "activeFocus", true)
            editor.enabled = false
            dialog.close()
            verify(!editor.activeFocus)
        }
    }
}
