import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"

Item {
    width: 700; height: 400
    Component { id: factory; AppMessageBar { width: 600 } }
    SignalSpy { id: details; signalName: "detailsRequested" }
    TestCase {
        name: "MessageBar"
        when: windowShown
        function test_long_error_stays_one_line() {
            const bar = createTemporaryObject(factory, parent, {message: "ASAP engine is not installed", kind: "error"})
            verify(bar)
            wait(0)
            const oneLine = bar.implicitHeight
            verify(oneLine > 0)
            compare(findChild(bar, "messageDetails").visible, false, "No details without fuller text")
            let traceback = "Traceback (most recent call last):"
            for (let i = 0; i < 50; i++) traceback += "\n  File \"worker.py\", line " + i + ", in <module>"
            bar.message = traceback
            bar.details = traceback
            wait(0)
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
