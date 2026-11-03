import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"

Item {
    width: 700; height: 400
    Component { id: factory; AppMessageBar { width: 600 } }
    TextField { id: editor; y: 200 }
    QtObject {
        id: firstDocument
        property bool busy: false
        property string stage: ""
        property string message: ""
    }
    QtObject {
        id: secondDocument
        property bool busy: false
        property string stage: ""
        property string message: "Second document ready"
    }
    Component {
        id: boundFactory
        AppMessageBar {
            width: 600
            property var controller: firstDocument
            busy: controller.busy
            workingText: controller.stage
            message: controller.message
        }
    }
    SignalSpy { id: cancelled; signalName: "cancelRequested" }
    SignalSpy { id: details; signalName: "detailsRequested" }
    TestCase {
        name: "MessageBar"
        when: windowShown
        function test_activity_keeps_focus_and_follows_document() {
            const bar = createTemporaryObject(boundFactory, parent)
            editor.forceActiveFocus()
            firstDocument.busy = true
            firstDocument.stage = "Preparing input"
            verify(bar.visible)
            compare(bar.Accessible.name, "Preparing input")
            const spinner = findChild(bar, "activitySpinner")
            verify(spinner.running)
            wait(150)
            verify(editor.activeFocus, "Status updates must not take keyboard focus")
            firstDocument.stage = "Aligning with MAFFT"
            compare(bar.firstLine, "Aligning with MAFFT")
            bar.controller = secondDocument
            verify(!spinner.running)
            compare(bar.firstLine, "Second document ready")
            // Late first-document updates must not overwrite the selected one.
            firstDocument.busy = false
            firstDocument.message = "First document done"
            compare(bar.firstLine, "Second document ready")
            bar.controller = firstDocument
            compare(bar.firstLine, "First document done")
        }
        function test_terminal_outcomes_data() {
            return [
                {tag: "success", kind: "done", message: "Saved results"},
                {tag: "failure", kind: "fail", message: "MAFFT failed"},
                {tag: "cancelled", kind: "notify", message: "Operation cancelled"}
            ]
        }
        function test_terminal_outcomes(data) {
            const bar = createTemporaryObject(factory, parent, {busy: true, workingText: "Preparing input"})
            cancelled.target = bar; cancelled.clear()
            const button = findChild(bar, "activityCancel")
            verify(button.visible)
            mouseClick(button)
            compare(cancelled.count, 1)
            // Cancellation stays working until the owner reports completion.
            verify(findChild(bar, "activitySpinner").running)
            bar.workingText = "Cancelling"
            compare(bar.firstLine, "Cancelling")
            bar.busy = false; bar.message = data.message; bar.kind = data.kind
            compare(bar.firstLine, data.message)
            verify(!findChild(bar, "activitySpinner").running)
            verify(!button.visible)
            bar.message = ""
            verify(!bar.visible)
        }
        function test_unsupported_cancel_has_no_action() {
            const bar = createTemporaryObject(factory, parent, {busy: true, cancellable: false, workingText: "Saving results"})
            verify(!findChild(bar, "activityCancel").visible)
            compare(bar.firstLine, "Saving results")
        }
        function test_long_error_stays_one_line() {
            const bar = createTemporaryObject(factory, parent, {message: "ASAP engine is not installed", kind: "error"})
            verify(bar)
            compare(findChild(bar, "messageDetails").visible, false, "No details without fuller text")
            // Compare messages with the same controls: showing Details may
            // legitimately increase the height to fit the button.
            bar.details = "Short diagnostic details"
            verify(waitForRendering(bar))
            verify(waitForPolish(bar))
            const oneLine = bar.implicitHeight
            verify(oneLine > 0)
            let traceback = "Traceback (most recent call last):"
            for (let i = 0; i < 50; i++) traceback += "\n  File \"worker.py\", line " + i + ", in <module>"
            bar.message = traceback
            bar.details = traceback
            verify(waitForRendering(bar))
            verify(waitForPolish(bar))
            compare(bar.implicitHeight, oneLine, "A 50-line error must not grow the bar")
            compare(findChild(bar, "messageText").lineCount, 1)
            const button = findChild(bar, "messageDetails")
            verify(button.visible)
            details.target = bar; details.clear()
            mouseClick(button)
            compare(details.count, 1)
        }
    }
}
