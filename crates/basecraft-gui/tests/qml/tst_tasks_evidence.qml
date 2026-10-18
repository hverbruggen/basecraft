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
