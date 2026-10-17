import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 800; height: 700
    Component { id: factory; DelimitationAnalysisDialog {} }
    SignalSpy { id: requests; signalName: "requested" }
    TextField { id: invoker; text: "Document" }
    SignalSpy { id: rejected; signalName: "rejected" }
    SignalSpy { id: accepted; signalName: "accepted" }
    TestCase {
        name: "DelimitationAnalysis"
        when: windowShown
        function test_escape_data() {
            const rows = []
            for (const method of ["gmyc", "asap", "ptp", "manual", "view"])
                for (const focus of ["text", "number", "button"])
                    rows.push({tag: method + "-" + focus, method, focus})
            return rows
        }
        function test_escape(data) {
            invoker.forceActiveFocus()
            const dialog = createTemporaryObject(factory, parent)
            requests.target = dialog; requests.clear()
            rejected.target = dialog; rejected.clear()
            accepted.target = dialog; accepted.clear()
            dialog.show(data.method, "/input.afa", "/input.nwk")
            tryCompare(dialog, "opened", true)
            wait(0)
            if (dialog.viewSetup) findChild(dialog, "manualTree").currentIndex = 4
            if (data.method === "ptp") findChild(dialog, "rootedTree").checked = true
            const field = data.focus === "button" ? dialog.standardButton(Dialog.Ok)
                : data.focus === "number" ? findChild(dialog, "randomSeed").contentItem
                : findChild(dialog, dialog.viewSetup ? "iqtreeModel" : "analysisInput")
            field.forceActiveFocus(Qt.TabFocusReason)
            tryCompare(field, "activeFocus", true)
            if (data.focus !== "button") { keyClick(Qt.Key_A, Qt.ControlModifier); keyClick("9") }
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            compare(rejected.count, 1)
            compare(accepted.count, 0)
            compare(requests.count, 0, "Escape cannot request an analysis or workspace")
            tryCompare(invoker, "activeFocus", true)
        }
        function test_methods_data() { return [{tag:"gmyc"}, {tag:"asap"}, {tag:"ptp"}, {tag:"manual"}] }
        function test_methods(data) {
            const dialog = createTemporaryObject(factory, parent)
            verify(dialog)
            requests.target = dialog
            requests.clear()
            dialog.show(data.tag, "/project/input.afa", "/project/input.nwk")
            tryCompare(dialog, "opened", true)
            verify(dialog.width <= parent.width)
            verify(dialog.height <= parent.height)
            if (data.tag === "gmyc") {
                compare(dialog.operation(), "gmyc_alignment_single")
                findChild(dialog, "gmycThresholds").currentIndex = 1
                compare(dialog.operation(), "gmyc_alignment_multiple")
                findChild(dialog, "gmycSource").currentIndex = 1
                compare(dialog.operation(), "gmyc_multiple")
                verify(!dialog.validInput)
                findChild(dialog, "rootedTree").checked = true
                verify(!dialog.validInput) // a requested deduplication is never silently discarded
                findChild(dialog, "deduplicateInput").checked = false
                verify(dialog.validInput)
            } else if (data.tag === "ptp") {
                compare(dialog.operation(), "ptp")
                verify(!dialog.validInput)
                findChild(dialog, "rootedTree").checked = true
                findChild(dialog, "ptpRates").currentIndex = 1
                compare(dialog.operation(), "mptp")
            } else if (data.tag === "asap") {
                compare(dialog.operation(), "asap")
                compare(dialog.parameters().replicates, 10000)
                compare(findChild(dialog, "asapView").currentIndex, 1)
            } else {
                findChild(dialog, "manualTree").currentIndex = 2
                verify(dialog.buildsTree)
                verify(dialog.validInput)
            }
            const operation = dialog.operation()
            dialog.accept()
            compare(requests.count, 1)
            compare(requests.signalArguments[0][0], operation)
            dialog.show(data.tag, "", "")
            tryCompare(dialog, "opened", true)
            dialog.reject()
            compare(requests.count, 1)
        }
        function test_missing_engine_blocks_run() {
            const dialog = createTemporaryObject(factory, parent)
            requests.target = dialog
            requests.clear()
            dialog.engineProblems = {asap: "ASAP engine is not installed", ptp: null, mptp: "mPTP is not installed"}
            dialog.show("asap", "/input.afa", "")
            tryCompare(dialog, "opened", true)
            const problem = findChild(dialog, "engineProblem")
            verify(problem.visible)
            compare(problem.text, "ASAP engine is not installed")
            verify(!dialog.validInput)
            verify(!dialog.standardButton(Dialog.Ok).enabled)
            dialog.reject()
            dialog.show("ptp", "", "/input.nwk")
            tryCompare(dialog, "opened", true)
            findChild(dialog, "rootedTree").checked = true
            verify(dialog.validInput, "single-rate PTP engine is available")
            findChild(dialog, "ptpRates").currentIndex = 1
            verify(!dialog.validInput)
            compare(findChild(dialog, "engineProblem").text, "mPTP is not installed")
            dialog.reject()
            compare(requests.count, 0)
        }
        function test_sampling_and_iqtree_data() {
            return [{tag:"asap"}, {tag:"manual"}, {tag:"view"}]
        }
        function test_sampling_and_iqtree(data) {
            const dialog = createTemporaryObject(factory, parent)
            requests.target = dialog
            requests.clear()
            dialog.show(data.tag, "/input.afa", "")
            tryCompare(dialog, "opened", true)
            const sampling = findChild(dialog, "deduplicateInput")
            if (data.tag === "asap") {
                compare(dialog.parameters().deduplicate, false)
                sampling.checked = true
                compare(dialog.parameters().deduplicate, true)
                findChild(dialog, "asapView").currentIndex = 3
            } else {
                verify(!sampling.visible)
                findChild(dialog, "manualTree").currentIndex = 4
            }
            verify(dialog.iqtreeDisplay)
            compare(dialog.parameters().iqtree_model, "MFP")
            findChild(dialog, "iqtreeModel").text = "GTR+G"
            compare(dialog.parameters().iqtree_model, "GTR+G")
            dialog.accept()
            compare(requests.count, 1)
            compare(requests.signalArguments[0][4], "iqtree")
        }
        function test_tree_deduplication_is_explicitly_unavailable_data() {
            return [{tag:"gmyc"}, {tag:"ptp"}]
        }
        function test_tree_deduplication_is_explicitly_unavailable(data) {
            const dialog = createTemporaryObject(factory, parent)
            dialog.show(data.tag, "", "/input.nwk")
            tryCompare(dialog, "opened", true)
            findChild(dialog, "rootedTree").checked = true
            verify(dialog.validInput)
            findChild(dialog, "deduplicateInput").checked = true
            verify(dialog.unsupportedDeduplication)
            verify(!dialog.validInput)
            compare(dialog.parameters().deduplicate, true)
            findChild(dialog, "deduplicateInput").checked = false
            verify(dialog.validInput)
            dialog.reject()
        }
        function test_manual_without_tree() {
            const dialog = createTemporaryObject(factory, parent)
            dialog.show("manual", "", "")
            tryCompare(dialog, "opened", true)
            compare(findChild(dialog, "manualTree").currentIndex, 0)
            verify(dialog.validInput)
            dialog.reject()
        }
    }
}
