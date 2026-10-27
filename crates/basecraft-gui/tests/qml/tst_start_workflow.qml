import QtQuick
import QtTest
import "../../qml"

Item {
    id: host
    width: 800; height: 600
    property string source
    function block(start) {
        const begin = source.indexOf(start)
        let end = source.indexOf("{", begin) + 1, depth = 1
        while (depth && end < source.length) {
            const c = source[end++]
            if (c === "{") depth++
            if (c === "}") depth--
        }
        return source.slice(begin, end)
    }
    TestCase {
        name: "StartWorkflow"
        when: windowShown
        function initTestCase() {
            const request = new XMLHttpRequest()
            request.open("GET", Qt.resolvedUrl("../../qml/Main.qml"), false)
            request.send()
            host.source = request.responseText
            verify(host.source.length > 0)
        }
        function harness() {
            return Qt.createQmlObject(`
                import QtQuick
                import "../../qml"
                Item {
                    id: window
                    property var current: ({identity: "unsaved current document"})
                    property var customization: ({shortcuts: {}, ribbon_hidden: []})
                    property var recentProjects: ["/previous"]
                    property string recentProjectsError: ""
                    property alias project: shell.project_dir
                    property bool directoryExists: true
                    property string rememberError: ""
                    property int rememberCount: 0
                    property alias savedRevision: documentRecord.savedRevision
                    property alias page: documentRecord.page
                    property alias methods: methodsCommand
                    property alias provenanceBusy: provenance.busy
                    QtObject { id: shell; property string project_dir: "/previous" }
                    QtObject { id: provenance; property bool busy: false }
                    QtObject {
                        id: preferences
                        function project_is_directory(path) { return window.directoryExists }
                        function remember_project(path) {
                            window.rememberCount++
                            return JSON.stringify({error: window.rememberError, projects: [path, "/previous"]})
                        }
                    }
                    QtObject {
                        id: documentRecord
                        property bool savedRevision: true
                        property int page: -1
                        function showPage(index) { page = index }
                    }
                    ${host.block("function selectProject(")}
                    AppCommand { ${host.block("id: methodsCommand")} }
                }`, host, Qt.resolvedUrl("startWorkflowHarness.qml"))
        }
        function test_recent_project_failures_preserve_context() {
            const scene = harness()
            const current = scene.current
            scene.directoryExists = false
            scene.selectProject("/missing")
            compare(scene.project, "/previous")
            compare(scene.current, current)
            compare(scene.rememberCount, 0)
            verify(scene.recentProjectsError.includes("/missing"))
            scene.directoryExists = true
            scene.rememberError = "Cannot save preferences"
            scene.selectProject("/unremembered")
            compare(scene.project, "/previous")
            compare(scene.current, current)
            compare(scene.recentProjects, ["/previous"])
            compare(scene.recentProjectsError, "Cannot save preferences")
            scene.rememberError = ""
            scene.selectProject("/new")
            compare(scene.project, "/new")
            compare(scene.recentProjects, ["/new", "/previous"])
            compare(scene.recentProjectsError, "")
            compare(scene.current, current)
            scene.destroy()
        }
        function test_methods_uses_current_revision_and_no_document_guard() {
            const scene = harness()
            verify(scene.methods.enabled)
            scene.methods.trigger()
            compare(scene.savedRevision, false)
            compare(scene.page, 1)
            scene.provenanceBusy = true
            verify(!scene.methods.enabled)
            scene.provenanceBusy = false
            scene.current = null
            verify(!scene.methods.enabled)
            verify(scene.methods.tooltip.includes("Open a document"))
            scene.destroy()
        }
    }
}
