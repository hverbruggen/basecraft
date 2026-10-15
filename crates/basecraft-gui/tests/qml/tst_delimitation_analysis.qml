import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 800; height: 700
    Component { id: factory; DelimitationAnalysisDialog {} }
    SignalSpy { id: requests; signalName: "requested" }
    TestCase {
        name: "DelimitationAnalysis"
        when: windowShown
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
