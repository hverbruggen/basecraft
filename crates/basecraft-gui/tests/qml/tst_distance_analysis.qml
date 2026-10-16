import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 800; height: 700
    QtObject {
        id: backend
        property bool busy: false
        property int calls: 0
        property string kind
        property var options
        function distance_action(k, p) { calls++; kind = k; options = JSON.parse(p) }
    }
    Component { id: dialogComponent; DistanceAnalysisDialog {} }
    TextField { id: invoker; text: "Document" }
    SignalSpy { id: rejected; signalName: "rejected" }
    SignalSpy { id: accepted; signalName: "accepted" }
    TestCase {
        name: "DistanceAnalysis"
        when: windowShown
        function test_escape_data() {
            const rows = []
            for (const kind of ["matrix", "nj", "upgma"])
                for (const focus of (kind === "matrix" ? ["number", "button"] : ["number", "button", "destination"]))
                    rows.push({tag: kind + "-" + focus, kind, focus})
            return rows
        }
        function test_escape(data) {
            invoker.forceActiveFocus()
            const d = createTemporaryObject(dialogComponent, parent, {kind: data.kind, target: backend})
            rejected.target = d; rejected.clear()
            accepted.target = d; accepted.clear()
            d.open()
            tryCompare(findChild(d, "distanceModel"), "activeFocus", true)
            if (data.focus === "destination") findChild(d, "distanceBootstrap").checked = true
            const field = data.focus === "number" ? findChild(d, "distanceOverlap").contentItem
                : data.focus === "destination" ? findChild(d, "bootstrapDestination") : d.standardButton(Dialog.Ok)
            field.forceActiveFocus(Qt.TabFocusReason)
            tryCompare(field, "activeFocus", true)
            if (data.focus === "number") { keyClick(Qt.Key_A, Qt.ControlModifier); keyClick("9") }
            const before = backend.calls
            keyClick(Qt.Key_Escape)
            tryCompare(d, "opened", false)
            compare(rejected.count, 1)
            compare(accepted.count, 0)
            compare(backend.calls, before, "Escape cannot launch a distance job")
            tryCompare(invoker, "activeFocus", true)
        }
        function test_analysis_data() { return [{tag: "matrix"}, {tag: "nj"}, {tag: "upgma"}] }
        function test_analysis(data) {
            const d = createTemporaryObject(dialogComponent, parent, {kind: data.tag, target: backend})
            verify(d)
            d.open()
            tryCompare(d, "opened", true)
            compare(d.parameters().model, data.tag === "matrix" ? "p" : "k2p")
            compare(findChild(d, "distanceMethod").visible, data.tag === "nj")
            compare(findChild(d, "distanceBootstrap").visible, data.tag !== "matrix")
            verify(!findChild(d, "bootstrapDestination").visible)
            verify(!("threads" in d.parameters()))
            verify(!("tree_threads" in d.parameters()))
            const before = backend.calls
            d.reject()
            compare(backend.calls, before)
            d.open()
            tryCompare(d, "opened", true)
            findChild(d, "distanceBootstrap").checked = true
            const p = d.parameters()
            if (data.tag === "matrix") verify(!("bootstrap" in p))
            else {
                compare(p.bootstrap, 1000)
                compare(p.seed, 1)
                compare(p.bootstrap_destination, "metadata")
                verify(findChild(d, "bootstrapDestination").visible)
                findChild(d, "bootstrapDestination").currentIndex = 1
                compare(d.parameters().bootstrap_destination, "working")
                findChild(d, "bootstrapDestination").currentIndex = 0
                if (data.tag === "nj") {
                    compare(p.method, "sorted")
                    compare(findChild(d, "distanceMethod").currentText, "sorted")
                }
            }
            d.accept()
            compare(backend.calls, before + 1)
            compare(backend.kind, data.tag)
            compare(backend.options, p)
        }
    }
}
