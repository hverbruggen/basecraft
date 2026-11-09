import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 800; height: 650
    QtObject { id: controller; property bool busy: false }
    TextField { id: invoker; text: "Previous document" }
    AddSequenceDialog { id: dialog; controller: controller; newDocument: true; onInputError: message => validationMessage = message }
    SignalSpy { id: requested; target: dialog; signalName: "createRequested" }
    SignalSpy { id: rejected; target: dialog; signalName: "rejected" }
    TestCase {
        name: "NewSequenceDialog"
        when: windowShown
        function cleanup() { dialog.close(); requested.clear(); rejected.clear() }
        function test_reuses_fields_hides_alignment_and_cancels() {
            invoker.forceActiveFocus()
            dialog.prepare("ACGT", "sample")
            tryCompare(dialog, "opened", true)
            compare(dialog.title, "New sequence")
            compare(findChild(dialog.contentItem, "additionMode").visible, false)
            tryCompare(findChild(dialog.contentItem, "sequenceName"), "activeFocus", true)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            compare(requested.count, 0)
            compare(rejected.count, 1)
            tryCompare(invoker, "activeFocus", true)
        }
        function test_validation_keeps_text_and_multiline_enter_does_not_submit() {
            dialog.prepare("ACGT", "")
            tryCompare(dialog, "opened", true)
            dialog.submit(false)
            verify(dialog.validationMessage.length > 0)
            compare(requested.count, 0)
            const name = findChild(dialog.contentItem, "sequenceName")
            const input = findChild(dialog.contentItem, "sequenceInput")
            name.text = "sample"
            input.forceActiveFocus()
            keyClick(Qt.Key_Return)
            compare(requested.count, 0)
            input.text = ">sample\nACGT"
            dialog.submit(false)
            compare(requested.count, 0)
            verify(dialog.validationMessage.includes("plain sequence"))
            input.text = "ACGT"
            dialog.submit(false)
            compare(requested.count, 1)
            compare(requested.signalArguments[0][0], "sample")
            compare(requested.signalArguments[0][1], "ACGT")
            // Backend failure remains editable; only successful creation accepts.
            dialog.validationMessage = "Invalid DNA residue"
            verify(dialog.opened)
            compare(input.text, "ACGT")
        }
    }
}
