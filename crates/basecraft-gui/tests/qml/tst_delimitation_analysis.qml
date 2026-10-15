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
