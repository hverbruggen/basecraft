import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 1000; height: 700
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
        name: "WorkspaceCreation"
        when: windowShown
        function initTestCase() {
            const request = new XMLHttpRequest()
            request.open("GET", Qt.resolvedUrl("../../qml/Main.qml"), false)
            request.send()
            host.source = request.responseText
            verify(host.source.length > 0)
        }
        function harness() {
            const functions = ["documentIsOpen", "retireDelimitationDocument", "requestDelimitation", "completeDelimitation"]
                .map(name => host.block("function " + name + "(")).join("\n")
            return Qt.createQmlObject(`
                import QtQuick
                import QtQuick.Controls.Basic
                import QtQuick.Layouts
                import "../../qml"
                Item {
                    id: window
                    width: 1000; height: 700
                    property var delimitationSource: ctl
                    property var delimitationDocument: sourceDoc
                    property var delimitationRequest: null
                    property var current: ctl
                    property string currentKind: "alignment"
                    property bool alignmentOpen: currentKind === "alignment"
                    property bool treeOpen: currentKind === "tree"
                    property string ribbonTabName: "Delimitation"
                    property alias controller: ctl
                    property alias sourceDocument: sourceDoc
                    property alias selectedIndex: tabs.currentIndex
                    property alias openCount: openFiles.count
                    property alias keyboardTarget: editor
                    property alias lastResult: docs.lastResult
                    QtObject { id: tabs; property int currentIndex: 0 }
                    QtObject {
                        id: ctl
                        signal workspaceCreated(string path)
                        property string file_path: "/source.afa"
                        property bool busy: false
                        property bool accept: true
                        property string message: ""
                        property bool bootstrap_running: false
                        property int submissions: 0
                        function create_delimitation(method) { submissions++; busy = accept; message = "Creating species workspace"; return accept }
                        function complete(path) { busy = false; workspaceCreated(path) }
                    }
                    Item {
                        id: sourceDoc
                        property var controller: ctl
                        property string path: "/source.afa"
                        property string kind: "alignment"
                        property var analyses: []
                        function startAnalysis(method, input, rooted, settings, display) { analyses = analyses.concat([{method, input, rooted, settings, display}]) }
                    }
                    Item { id: otherDoc; property string path: "/other.afa" }
                    QtObject {
                        id: docs
                        property var rows: [sourceDoc, otherDoc]
                        property var lastResult: null
                        function itemAt(index) { return rows[index] || null }
                    }
                    QtObject {
                        id: openFiles
                        readonly property int count: docs.rows.length
                        function get(index) { return {path: docs.rows[index].path} }
                        function append(row) {
                            docs.lastResult = resultFactory.createObject(window, {path: row.path})
                            docs.rows = docs.rows.concat([docs.lastResult])
                        }
                    }
                    Component {
                        id: resultFactory
                        Item {
                            id: resultDoc
                            property string path: ""
                            property string kind: "delimitation"
                            property bool draft: false
                            property var analyses: []
                            property QtObject controller: QtObject {
                                property string file_path: resultDoc.path
                                property string linked_alignment: "/frozen-input.afa"
                            }
                            function startAnalysis(method, input, rooted, settings, display) { analyses = analyses.concat([{method, input, rooted, settings, display}]) }
                        }
                    }
                    TextField { id: editor; y: 300 }
                    function closeSource() {
                        retireDelimitationDocument(sourceDoc)
                        docs.rows = docs.rows.slice(1)
                    }
                    ${functions}
                    ${host.block("Connections {\n        target: window.delimitationRequest")}
                    ${host.block("AppMessageBar {\n                objectName: \"documentActivity\"")}
                }`, host, Qt.resolvedUrl("workspaceCreationHarness.qml"))
        }
        function test_submission_keeps_settings_and_background_identity() {
            failOnWarning(/TypeError|ReferenceError|Binding loop/)
            const scene = harness()
            scene.keyboardTarget.forceActiveFocus()
            scene.requestDelimitation("asap", "/source.afa", false, '{"replicates":123}', "nj")
            verify(scene.delimitationRequest !== null)
            compare(scene.controller.submissions, 1)
            compare(scene.openCount, 2)
            const activity = findChild(scene, "documentActivity")
            verify(activity.busy)
            compare(activity.firstLine, "Creating species workspace")
            scene.selectedIndex = 1
            scene.current = null
            scene.currentKind = "sequence"
            // Duplicate/different form submissions cannot retarget a pending job.
            scene.requestDelimitation("ptp", "/changed", true, "{}", "existing")
            compare(scene.controller.submissions, 1)
            scene.controller.complete("/result.spart")
            tryVerify(() => scene.lastResult && scene.lastResult.analyses.length === 1)
            compare(scene.lastResult.analyses[0], {method: "asap", input: "/frozen-input.afa", rooted: false, settings: '{"replicates":123}', display: "nj"})
            compare(scene.delimitationRequest, null)
            compare(scene.selectedIndex, 1)
            verify(scene.keyboardTarget.activeFocus)
            compare(scene.ribbonTabName, "Delimitation")
            scene.destroy()
        }
        function test_tree_request_keeps_explicit_input() {
            const scene = harness()
            scene.sourceDocument.kind = "tree"
            scene.currentKind = "tree"
            scene.requestDelimitation("ptp", "/chosen-tree.nwk", true, '{"rates":"shared"}', "existing")
            scene.selectedIndex = 1
            scene.controller.complete("/tree-result.spart")
            tryVerify(() => scene.lastResult && scene.lastResult.analyses.length === 1)
            compare(scene.lastResult.analyses[0], {method: "ptp", input: "/chosen-tree.nwk", rooted: true, settings: '{"rates":"shared"}', display: "existing"})
            compare(scene.selectedIndex, 1)
            scene.destroy()
        }
        function test_failure_rejection_and_closed_source() {
            const scene = harness()
            scene.controller.accept = false
            scene.requestDelimitation("ptp", "/tree.nwk", true, "{}", "existing")
            compare(scene.delimitationRequest, null)
            scene.controller.accept = true
            scene.requestDelimitation("ptp", "/tree.nwk", true, "{}", "existing")
            const oldRequest = scene.delimitationRequest
            scene.controller.complete("")
            compare(scene.delimitationRequest, null)
            compare(scene.openCount, 2)
            scene.requestDelimitation("asap", "/source.afa", false, "{}", "none")
            const newerRequest = scene.delimitationRequest
            scene.completeDelimitation(oldRequest, "/stale.spart")
            compare(scene.delimitationRequest, newerRequest)
            scene.closeSource()
            compare(scene.delimitationRequest, null)
            scene.controller.complete("/late.spart")
            compare(scene.openCount, 1)
            compare(scene.lastResult, null)
            scene.destroy()
        }
        function test_replaced_source_and_existing_output_are_untouched() {
            const scene = harness()
            scene.requestDelimitation("asap", "/source.afa", false, "{}", "none")
            scene.controller.file_path = "/replaced.afa"
            scene.controller.complete("/late.spart")
            compare(scene.openCount, 2)
            scene.controller.file_path = "/source.afa"
            scene.requestDelimitation("ptp", "/tree.nwk", true, "{}", "existing")
            scene.controller.complete("/other.afa")
            compare(scene.openCount, 2)
            compare(scene.lastResult, null)
            scene.destroy()
        }
        function test_existing_workspace_uses_captured_document() {
            const scene = harness()
            scene.sourceDocument.kind = "delimitation"
            scene.selectedIndex = 1
            scene.current = null
            scene.currentKind = "sequence"
            scene.requestDelimitation("ptp", "/tree.nwk", true, '{"rate":"shared"}', "existing")
            compare(scene.controller.submissions, 0)
            compare(scene.sourceDocument.analyses, [{method: "ptp", input: "/tree.nwk", rooted: true, settings: '{"rate":"shared"}', display: "existing"}])
            compare(scene.selectedIndex, 1)
            scene.destroy()
        }
        function test_genome_annotation_status_follows_current_document() {
            const scene = harness()
            scene.currentKind = "sequence"
            scene.controller.busy = true
            scene.controller.message = "Running Prodigal"
            const activity = findChild(scene, "documentActivity")
            verify(activity.busy)
            compare(activity.firstLine, "Running Prodigal")
            verify(!activity.cancellable)
            scene.current = null
            verify(!activity.busy)
            scene.current = scene.controller
            scene.controller.busy = false
            verify(!activity.busy)
            scene.destroy()
        }
    }
}
