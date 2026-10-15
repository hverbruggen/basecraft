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
    TestCase {
        name: "DistanceAnalysis"
        when: windowShown
        function test_analysis_data() { return [{tag: "matrix"}, {tag: "nj"}, {tag: "upgma"}] }
        function test_analysis(data) {
            const d = createTemporaryObject(dialogComponent, parent, {kind: data.tag, target: backend})
            verify(d)
            d.open()
            tryCompare(d, "opened", true)
            compare(d.parameters().model, data.tag === "matrix" ? "p" : "k2p")
            compare(findChild(d, "distanceMethod").visible, data.tag === "nj")
            compare(findChild(d, "distanceBootstrap").visible, data.tag !== "matrix")
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
