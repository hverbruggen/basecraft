import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

TestCase {
    id: testCase
    name: "TaskEvidenceResults"
    width: 800
    height: 700
    when: windowShown
    Component { id: dialogComponent; TasksDialog {} }
    SignalSpy { id: openSpy; signalName: "openDocument" }
    SignalSpy { id: copySpy; signalName: "copyText" }
    Component {
        id: serviceComponent
        QtObject {
            property string notifications: '[{"time":"now","kind":"notify","message":"finished","document":""}]'
            property string retained: JSON.stringify({jobs: [{id: "retained", operation: "tree", status: "completed", source: {path: "/a.afa"}, outputs: {}, primary_output: "/tree.nwk"}]})
            function retained_jobs() { return retained }
            function clear_notifications() { notifications = "[]" }
        }
    }
    function test_active_states_data() {
        return [
            {tag: "queued", status: "queued", active: true, cancel: true},
            {tag: "running", status: "running", active: true, cancel: true},
            {tag: "cancelling", status: "cancelling", active: true, cancel: false},
            {tag: "finalizing", status: "finalizing", active: true, cancel: false},
            {tag: "partial", status: "partial", active: false, cancel: false},
            {tag: "unconfirmed", status: "Previous owner; completion unconfirmed", active: false, cancel: false}
        ]
    }
    function test_active_states(data) {
        const dialog = createTemporaryObject(dialogComponent, testCase)
        dialog.jobData = {jobs: [{id: "run", operation: "tree", status: data.status, cancellable: true, source: {path: "/a.afa"}, outputs: {}}]}
        dialog.open()
        tryCompare(dialog, "opened", true)
        const progress = findChild(dialog.contentItem, "taskActivity")
        const cancel = findChild(dialog.contentItem, "cancelTask")
        verify(progress)
        verify(cancel)
        compare(progress.visible, data.active)
        compare(cancel.visible, data.active)
        compare(cancel.enabled, data.cancel)
        dialog.jobData = {jobs: [{id: "run", operation: "tree", status: data.status, cancellable: false, source: {path: "/a.afa"}, outputs: {}}]}
        wait(0)
        compare(findChild(dialog.contentItem, "cancelTask").enabled, false)
        dialog.close()
    }
    function test_clear_notifications_retains_tasks() {
        const service = createTemporaryObject(serviceComponent, testCase)
        const dialog = createTemporaryObject(dialogComponent, testCase, {service: service})
        dialog.open()
        tryCompare(dialog, "opened", true)
        compare(dialog.notices.length, 1)
        const before = service.retained
        const clear = findChild(dialog.contentItem, "clearTaskNotifications")
        verify(clear)
        mouseClick(clear)
        compare(dialog.notices.length, 0)
        findChild(dialog.contentItem, "previousRuns").checked = true
        compare(service.retained, before)
        compare(dialog.jobData.jobs[0].primary_output, "/tree.nwk")
        dialog.close()
    }
    function test_copy_error_takes_the_main_line() {
        const dialog = createTemporaryObject(dialogComponent, testCase)
        const trace = "Traceback (most recent call last):\n  File \"x.py\", line 3\nRuntimeError: GMYC requires positive event ages\n"
        compare(dialog.mainMessage(trace), "RuntimeError: GMYC requires positive event ages")
        compare(dialog.mainMessage("Plain failure\nmore detail"), "Plain failure")
        dialog.jobData = {jobs: [{operation: "delimitation", status: "failed", source: {path: "/a.afa"}, diagnostics: trace, outputs: {}}]}
        copySpy.target = dialog
        dialog.open()
        tryCompare(dialog, "opened", true)
        tryVerify(() => findChild(dialog.contentItem, "copyTaskError") !== null)
        const button = findChild(dialog.contentItem, "copyTaskError")
        tryCompare(button, "visible", true)
        mouseClick(button)
        compare(copySpy.signalArguments[0][0], "RuntimeError: GMYC requires positive event ages")
        dialog.close()
    }
    function test_primary_output_wins_over_hidden_supporting_artifacts() {
        const dialog = createTemporaryObject(dialogComponent, testCase)
        dialog.jobData = {jobs: [{operation: "tree", status: "completed", source: {path: "/work/input.afa"}, primary_output: "/work/tree.nwk", outputs: {"/work/.basecraft/log.json": "hash", "/work/tree.nwk": "hash"}}]}
        openSpy.target = dialog
        dialog.open()
        tryCompare(dialog, "opened", true)
        tryVerify(function() { return findChild(dialog.contentItem, "openTaskResult") !== null })
        const button = findChild(dialog.contentItem, "openTaskResult")
        verify(button)
        tryCompare(button, "visible", true)
        mouseClick(button)
        compare(openSpy.signalArguments[0][0], "/work/tree.nwk")
        dialog.jobData = {jobs: [{operation: "tree", status: "failed", source: {path: "/work/input.afa"}, primary_output: null, outputs: {"/work/.basecraft/bootstrap.partial.nex": "hash"}}]}
        wait(20)
        tryCompare(findChild(dialog.contentItem, "openTaskResult"), "visible", false)
        dialog.close()
    }
}
