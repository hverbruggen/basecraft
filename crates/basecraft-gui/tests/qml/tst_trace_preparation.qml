import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 900; height: 700
    property string source: ""
    function block(marker) {
        const start = source.indexOf(marker)
        let end = source.indexOf("{", start) + 1, depth = 1
        while (depth && end < source.length) {
            const c = source[end++]
            if (c === "{") depth++
            if (c === "}") depth--
        }
        return source.slice(start, end)
    }
    TestCase {
        name: "TracePreparation"
        when: windowShown
        function initTestCase() {
            const request = new XMLHttpRequest()
            request.open("GET", Qt.resolvedUrl("../../qml/SangerView.qml"), false)
            request.send()
            host.source = request.responseText
            verify(host.source.length > 0)
        }
        function harness() {
            // Execute production routing and failure controls with a deterministic
            // backend. Actual controller cancellation/session preservation is
            // covered separately in application_bootstrap.
            return Qt.createQmlObject(`
                import QtQuick
                import QtQuick.Controls.Basic
                import QtQuick.Layouts
                import "../../qml"
                Item {
                    id: doc
                    width: 900; height: 700
                    property bool sourceInitialized: false
                    property var returnAlignment: ({id: "existing target"})
                    property string reviewReference: "existing reference"
                    property var failedReview: null
                    property string preparationError: ""
                    property alias backend: ctl
                    QtObject {
                        id: ctl
                        property bool succeed: false
                        property bool review_mode: true
                        property string message: "No sufficient trace overlap"
                        property var requests: []
                        property int opens: 0
                        property int singles: 0
                        property var placements: []
                        function prepare_reads(paths, cutoff) {
                            requests = requests.concat([{paths: JSON.parse(paths), cutoff: cutoff}])
                            return succeed
                        }
                        function open_file(path) { opens++; return true }
                        function single_review(cutoff) { singles++; return true }
                        function orient_review(reference) { return true }
                        function place_alignment(reference) { placements = placements.concat([reference]) }
                    }
                    ${host.block("function retryReview(")}
                    ${host.block("function startReview(")}
                    ${host.block("Pane {\n        id: preparationFailure")}
                }`, host, Qt.resolvedUrl("tracePreparationHarness.qml"))
        }
        function test_failed_pair_keeps_context_and_error_until_retry() {
            failOnWarning(/ReferenceError|TypeError|Binding loop/)
            const scene = harness()
            const oldTarget = scene.returnAlignment
            const target = {id: "requested target"}
            const paths = ["/forward.ab1", "/reverse.ab1"]
            scene.startReview(paths, "requested reference", target, 35)
            paths[0] = "/changed.ab1"
            compare(scene.backend.opens, 0)
            compare(scene.returnAlignment, oldTarget)
            compare(scene.reviewReference, "existing reference")
            compare(scene.backend.placements, [])
            compare(scene.failedReview.paths, ["/forward.ab1", "/reverse.ab1"])
            compare(scene.failedReview.target, target)
            const panel = findChild(scene, "tracePreparationFailure")
            verify(panel.visible)
            scene.backend.message = "Computed calls retained"
            compare(scene.preparationError, "No sufficient trace overlap")
            findChild(scene, "retryTraceReview").clicked()
            compare(scene.backend.requests.length, 2)
            compare(scene.backend.requests[1], {paths: ["/forward.ab1", "/reverse.ab1"], cutoff: 35})
            verify(panel.visible)
            scene.backend.succeed = true
            findChild(scene, "retryTraceReview").clicked()
            compare(scene.failedReview, null)
            compare(scene.preparationError, "")
            verify(!panel.visible)
            compare(scene.returnAlignment, target)
            compare(scene.reviewReference, "requested reference")
            compare(scene.backend.placements, ["requested reference"])
            compare(scene.backend.opens, 0)
            scene.destroy()
        }
        function test_successful_single_and_pair_routing() {
            const scene = harness()
            scene.backend.succeed = true
            scene.startReview(["/single.ab1"], "", null)
            compare(scene.backend.opens, 1)
            compare(scene.backend.singles, 0)
            scene.startReview(["/single.ab1"], "", null, 35)
            compare(scene.backend.singles, 1)
            const target = {id: "alignment"}
            scene.startReview(["/one.ab1", "/two.ab1"], "reference", target)
            compare(scene.backend.requests[0], {paths: ["/one.ab1", "/two.ab1"], cutoff: 20})
            compare(scene.backend.opens, 2)
            compare(scene.returnAlignment, target)
            compare(scene.backend.placements, ["reference"])
            scene.destroy()
        }
        function test_dismiss_acknowledges_failure() {
            const scene = harness()
            scene.startReview(["/one.ab1", "/two.ab1"], "", null)
            const panel = findChild(scene, "tracePreparationFailure")
            verify(panel.visible)
            // Dismiss acknowledges the request; no automatic single-read action.
            findChild(scene, "dismissTraceFailure").clicked()
            compare(scene.failedReview, null)
            verify(!panel.visible)
            compare(scene.backend.opens, 0)
            scene.destroy()
        }
    }
}
