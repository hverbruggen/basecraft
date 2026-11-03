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
        name: "TraceCoordinates"
        when: windowShown
        function initTestCase() {
            const request = new XMLHttpRequest()
            request.open("GET", Qt.resolvedUrl("../../qml/SangerView.qml"), false)
            request.send()
            host.source = request.responseText
            verify(host.source.length > 0)
        }
        function harness() {
            return Qt.createQmlObject(`
                import QtQuick
                import QtQuick.Controls.Basic
                import QtQuick.Layouts
                import "../../qml"
                Item {
                    id: doc
                    width: 900; height: 700
                    property bool working: false
                    property alias backend: ctl
                    property alias contentY: viewport.contentY
                    property alias canvas: view
                    Item { id: traceActivity; height: 0 }
                    Item { id: viewport; property real contentY: 100 }
                    FocusScope { id: view }
                    QtObject {
                        id: ctl
                        property bool review_mode: true
                        property string status: "3 unresolved unmasked · 90 unmasked columns · 10 masked · 2 reads"
                        property string review_details: "Reference alignment column 4\\nDraft consensus column 2\\nRead 2 native event 10"
                        property bool accept: true
                        property var requests: []
                        function go_to_column(reference, text) {
                            requests = requests.concat([{reference: reference, text: text}])
                            return accept
                        }
                    }
                    ${host.block("function goToColumn(")}
                    ${host.block("Pane {\n        id: reviewNavigation")}
                }`, host, Qt.resolvedUrl("traceCoordinatesHarness.qml"))
        }
        function test_navigation_routes_explicit_system_and_restores_canvas_on_success() {
            failOnWarning(/ReferenceError|TypeError|Binding loop/)
            const scene = harness()
            const input = findChild(scene, "reviewCoordinateInput")
            const system = findChild(scene, "reviewCoordinateSystem")
            input.text = "342"
            scene.goToColumn()
            compare(scene.backend.requests[0], {reference: true, text: "342"})
            compare(scene.contentY, 0)
            verify(scene.canvas.activeFocus)
            system.currentIndex = 1
            input.text = "1154"
            input.accepted()
            compare(scene.backend.requests[1], {reference: false, text: "1154"})
            scene.destroy()
        }
        function test_failed_navigation_and_working_preserve_view() {
            const scene = harness()
            scene.backend.accept = false
            findChild(scene, "reviewCoordinateInput").text = "outside"
            scene.goToColumn()
            compare(scene.contentY, 100)
            scene.working = true
            scene.goToColumn()
            compare(scene.backend.requests.length, 1)
            scene.destroy()
        }
        function test_evidence_toggle_and_review_visibility() {
            const scene = harness()
            const panel = findChild(scene, "reviewNavigation")
            const details = findChild(scene, "reviewEvidence")
            verify(details.visible)
            verify(panel.height > 130)
            findChild(scene, "reviewEvidenceToggle").checked = false
            verify(!details.visible)
            tryVerify(() => panel.height < 130)
            scene.backend.review_mode = false
            compare(panel.height, 0)
            verify(!panel.visible)
            scene.destroy()
        }
    }
}
