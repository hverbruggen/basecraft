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
            // Run production routing/presentation with controlled async completion.
            // Backend tests own cancellation and stale request retirement itself.
            const functions = ["retryReview", "failReview", "finishReview", "submitReviewStage", "reviewStageFinished", "cancelWork", "startReview"]
                .map(name => host.block("function " + name + "(")).join("\n")
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
                    property var pendingReview: null
                    property bool primerPending: false
                    readonly property bool working: ctl.busy || ctl.calling || pendingReview !== null
                    property var failedReview: null
                    property string preparationError: ""
                    property alias backend: ctl
                    property alias primerProposal: primerDialog.proposal
                    Item { id: primerDialog; property string proposal: "" }
                    QtObject {
                        id: ctl
                        property bool accept: true
                        property bool review_mode: true
                        property bool busy: false
                        property bool calling: false
                        property string message: ""
                        property var requests: []
                        property int opens: 0
                        property int cancels: 0
                        function submit(stage, data) {
                            requests = requests.concat([{stage: stage, data: data}])
                            busy = accept
                            message = accept ? (stage === "prepare" || stage === "single" ? "Processing AB1 with Tracy" : "Aligning with MAFFT") : "Submission failed"
                            return accept
                        }
                        function prepare_reads(paths, cutoff) { return submit("prepare", {paths: JSON.parse(paths), cutoff: cutoff}) }
                        function single_review(cutoff) { return submit("single", cutoff) }
                        function open_file(path) { opens++; return true }
                        function orient_review(reference) { return submit("orient", reference) }
                        function place_alignment(reference) { return submit("place", reference) }
                        function cancel_stage() { cancels++ }
                        function cancel_calls() { calling = false }
                    }
                    function complete(stage, success, payload = "") {
                        ctl.busy = false
                        ctl.message = success ? "Stage done" : "Cancelled or failed"
                        doc.reviewStageFinished(stage, success, payload)
                    }
                    ${functions}
                    ${host.block("Pane {\n        id: preparationFailure")}
                    ${host.block("AppMessageBar {\n        id: traceActivity")}
                }`, host, Qt.resolvedUrl("tracePreparationHarness.qml"))
        }
        function test_slow_pair_advances_only_on_completion() {
            failOnWarning(/ReferenceError|TypeError|Binding loop/)
            const scene = harness()
            const oldTarget = scene.returnAlignment
            const target = {id: "requested target"}
            const paths = ["/forward.ab1", "/reverse.ab1"]
            scene.startReview(paths, "requested reference", target, 35)
            paths[0] = "/changed.ab1"
            const activity = findChild(scene, "traceActivity")
            verify(activity.visible)
            compare(activity.firstLine, "Processing AB1 with Tracy")
            compare(scene.returnAlignment, oldTarget)
            compare(scene.reviewReference, "existing reference")
            wait(250)
            compare(scene.backend.requests.length, 1)
            verify(findChild(activity, "activitySpinner").running)
            // Duplicate submission cannot replace the in-flight context.
            scene.startReview(["other"], "other", null)
            compare(scene.pendingReview.paths, ["/forward.ab1", "/reverse.ab1"])
            scene.complete("prepare", true)
            compare(scene.backend.requests[1], {stage: "orient", data: "requested reference"})
            compare(activity.firstLine, "Aligning with MAFFT")
            scene.complete("orient", true)
            compare(scene.backend.requests[2], {stage: "place", data: "requested reference"})
            compare(scene.returnAlignment, oldTarget)
            scene.complete("place", true)
            compare(scene.returnAlignment, target)
            compare(scene.reviewReference, "requested reference")
            compare(scene.pendingReview, null)
            verify(!activity.visible)
            scene.destroy()
        }
        function test_failure_retains_request_and_retry() {
            const scene = harness()
            const oldTarget = scene.returnAlignment
            scene.startReview(["one", "two"], "reference", {id: "new"}, 35)
            scene.complete("prepare", true)
            scene.complete("orient", false)
            compare(scene.returnAlignment, oldTarget)
            compare(scene.reviewReference, "existing reference")
            compare(scene.failedReview.cutoff, 35)
            const panel = findChild(scene, "tracePreparationFailure")
            verify(panel.visible)
            scene.backend.message = "Unrelated notification"
            compare(scene.preparationError, "Cancelled or failed")
            findChild(scene, "retryTraceReview").clicked()
            compare(scene.backend.requests[2], {stage: "prepare", data: {paths: ["one", "two"], cutoff: 35}})
            verify(!panel.visible)
            scene.complete("prepare", true)
            scene.complete("orient", true)
            scene.complete("place", true)
            compare(scene.failedReview, null)
            compare(scene.preparationError, "")
            scene.destroy()
        }
        function test_cancel_and_stale_stage_do_not_advance() {
            const scene = harness()
            const oldTarget = scene.returnAlignment
            scene.startReview(["one", "two"], "reference", null)
            scene.reviewStageFinished("place", true, "") // Wrong/stale stage.
            compare(scene.pendingReview.stage, "prepare")
            scene.cancelWork()
            compare(scene.backend.cancels, 1)
            // Even a success racing with Cancel cannot start orientation.
            scene.complete("prepare", true)
            compare(scene.backend.requests.length, 1)
            compare(scene.returnAlignment, oldTarget)
            verify(!findChild(scene, "traceActivity").visible)
            scene.reviewStageFinished("prepare", true, "")
            compare(scene.returnAlignment, oldTarget)
            findChild(scene, "dismissTraceFailure").clicked()
            compare(scene.failedReview, null)
            scene.destroy()
        }
        function test_single_and_submission_failure() {
            const scene = harness()
            scene.startReview(["single"], "", null)
            compare(scene.backend.opens, 1)
            compare(scene.backend.requests.length, 0)
            scene.startReview(["single"], "", null, 35)
            compare(scene.pendingReview.stage, "single")
            scene.complete("single", true)
            compare(scene.pendingReview, null)
            scene.backend.accept = false
            scene.startReview(["one", "two"], "", null)
            compare(scene.pendingReview, null)
            compare(scene.preparationError, "Submission failed")
            verify(!scene.working)
            scene.destroy()
        }
        function test_deferred_start_keeps_document_identity() {
            const request = new XMLHttpRequest()
            request.open("GET", Qt.resolvedUrl("../../qml/Main.qml"), false)
            request.send()
            const originalSource = host.source
            host.source = request.responseText
            const routing = host.block("function startTraceReview(")
            host.source = originalSource
            const scene = Qt.createQmlObject(`
                import QtQuick
                Item {
                    id: window
                    property string ribbonTabName: "File"
                    property alias selectedIndex: tabs.currentIndex
                    property alias firstStarts: first.starts
                    property alias secondStarts: second.starts
                    QtObject { id: tabs; property int currentIndex: 0 }
                    QtObject { id: openFiles; property int count: 2; function get(i) { return {path: i === 0 ? "one" : "two"} } }
                    QtObject { id: docs; function itemAt(i) { return i === 0 ? first : second } }
                    QtObject {
                        id: first
                        property string kind: "trace"
                        property bool working: false
                        property var controller: ({dirty: false})
                        property int starts: 0
                        function startReview(paths, reference, target, cutoff) { starts++ }
                    }
                    QtObject { id: second; property int starts: 0; function startReview(paths, reference, target, cutoff) { starts++ } }
                    function openPath(path) { tabs.currentIndex = 0 }
                    ${routing}
                }`, host, Qt.resolvedUrl("traceStartHarness.qml"))
            scene.startTraceReview(["one"], "", null)
            scene.selectedIndex = 1 // User switches tabs before deferred submission.
            tryCompare(scene, "firstStarts", 1)
            compare(scene.secondStarts, 0)
            compare(scene.selectedIndex, 1)
            scene.destroy()
        }
        function test_primer_payload_is_only_applied_to_pending_search() {
            const scene = harness()
            scene.reviewStageFinished("primers", true, "stale")
            compare(scene.primerProposal, "")
            scene.primerPending = true
            scene.reviewStageFinished("primers", true, '{"hits":[]}')
            compare(scene.primerProposal, '{"hits":[]}')
            compare(scene.primerPending, false)
            scene.destroy()
        }
    }
}
