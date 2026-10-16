import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"

Item {
    id: host
    width: 800; height: 640
    property int documentEscapes: 0
    TextField { id: invoker; text: "Document" }
    Shortcut { sequence: "Escape"; onActivated: host.documentEscapes++ }
    AppContentDialog {
        id: dialog
        title: "Pending edits"
        standardButtons: Dialog.Ok | Dialog.Cancel
        body: ColumnLayout {
            TextField { id: textField; text: "Pending text" }
            TextArea { id: textArea; text: "Pending multi-line text" }
            SpinBox { id: numberField; editable: true; value: 5 }
            AppComboBox { id: combo; model: ["First", "Second"] }
        }
    }
    AppFormDialog {
        id: confirmation
        title: "Save changes?"
        standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
        property bool quitPending: true
        property int saves: 0
        property int discards: 0
        onRejected: quitPending = false
        onAccepted: saves++
        onDiscarded: discards++
        Label { text: "Unsaved document" }
    }
    SignalSpy { id: confirmationRejected; target: confirmation; signalName: "rejected" }
    SignalSpy { id: rejected; target: dialog; signalName: "rejected" }
    SignalSpy { id: accepted; target: dialog; signalName: "accepted" }
    TestCase {
        name: "DialogEscape"
        when: windowShown
        function init() {
            host.documentEscapes = 0
            invoker.forceActiveFocus()
            tryCompare(invoker, "activeFocus", true)
            rejected.clear()
            accepted.clear()
        }
        function cleanup() { combo.popup.close(); dialog.close(); confirmation.close(); dialog.modal = true }
        function test_confirmation_rejects_and_runs_cancel_handler() {
            confirmation.open()
            tryCompare(confirmation.standardButton(Dialog.Cancel), "activeFocus", true)
            confirmationRejected.clear()
            keyClick(Qt.Key_Escape)
            tryCompare(confirmation, "opened", false)
            compare(confirmationRejected.count, 1)
            verify(!confirmation.quitPending)
            compare(confirmation.saves, 0)
            compare(confirmation.discards, 0)
            tryCompare(invoker, "activeFocus", true)
        }
        function test_close_only_rejects() {
            dialog.standardButtons = Dialog.Close
            dialog.open()
            tryCompare(dialog, "opened", true)
            wait(0)
            dialog.standardButton(Dialog.Close).forceActiveFocus()
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            compare(rejected.count, 1)
            compare(accepted.count, 0)
            tryCompare(invoker, "activeFocus", true)
            dialog.standardButtons = Dialog.Ok | Dialog.Cancel
        }
        function test_nonmodal() {
            dialog.modal = false
            dialog.open()
            tryCompare(dialog, "opened", true)
            wait(0)
            textField.forceActiveFocus()
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            compare(rejected.count, 1)
            compare(host.documentEscapes, 0)
        }
        function test_reject_data() {
            return [{tag: "text", control: textField},
                {tag: "multi-line", control: textArea},
                {tag: "number", control: numberField.contentItem},
                {tag: "button", control: dialog.standardButton(Dialog.Ok)}]
        }
        function test_reject(data) {
            dialog.open()
            tryCompare(dialog, "opened", true)
            wait(0)
            data.control.forceActiveFocus(Qt.TabFocusReason)
            tryCompare(data.control, "activeFocus", true)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            compare(rejected.count, 1)
            compare(accepted.count, 0)
            compare(host.documentEscapes, 0)
            tryCompare(invoker, "activeFocus", true)
        }
        function test_nested_combo() {
            dialog.open()
            tryCompare(dialog, "opened", true)
            wait(0)
            combo.forceActiveFocus()
            combo.popup.open()
            tryCompare(combo.popup, "opened", true)
            keyClick(Qt.Key_Escape)
            tryCompare(combo.popup, "opened", false)
            verify(dialog.opened)
            compare(rejected.count, 0)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            compare(rejected.count, 1)
            compare(accepted.count, 0)
            compare(host.documentEscapes, 0)
            tryCompare(invoker, "activeFocus", true)
        }
    }
}
