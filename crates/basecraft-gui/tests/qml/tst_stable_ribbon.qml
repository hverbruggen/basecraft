import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 1200; height: 800
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
        name: "StableRibbon"
        when: windowShown
        function init() {
            failOnWarning(/Binding loop|TypeError|ReferenceError/)
        }
        function initTestCase() {
            const request = new XMLHttpRequest()
            request.open("GET", Qt.resolvedUrl("../../qml/Main.qml"), false)
            request.send()
            host.source = request.responseText
            verify(host.source.length > 0)
        }
        function productionRibbon() {
            const ribbon = host.block("Ribbon {\n            id: ribbon")
            const components = host.source.slice(host.source.indexOf("    component RibbonButton:"), host.source.indexOf("    header: ColumnLayout"))
            const names = [...new Set((ribbon + components).match(/\b\w+Command\b/g))].filter(n => n !== "registerRibbonCommand")
            const declarations = names.map(n => `AppCommand { id: ${n}; commandId: "test.${n}"; text: "${n}" }`).join("\n")
            const tabsStart = host.source.indexOf("    readonly property var ribbonTabs:")
            const tabsEnd = host.source.indexOf("    // Only the wrapped", tabsStart)
            return Qt.createQmlObject(`
                import QtQuick
                import QtQuick.Controls.Basic
                import QtQuick.Layouts
                import "../../qml"
                Item {
                    id: window; width: 1200; height: 700
                    property real ribbonSizeFactor: 1
                    property real baseFontPixelSize: 12
                    property string currentKind: ""
                    property string currentPath: "/test"
                    property string ribbonTabName: "File"
                    property var current: null
                    property var currentView: null
                    property var speciesView: null
                    property var speciesTree: null
                    property var traceDocument: null
                    property bool alignmentOpen: currentKind === "alignment"
                    property bool treeOpen: currentKind === "tree"
                    property bool delimitationOpen: currentKind === "delimitation"
                    property bool traceOpen: currentKind === "trace"
                    property bool alignmentInsert: true
                    property bool showGc: true
                    property var customization: ({shortcuts: {}, ribbon_hidden: []})
                    property var recentProjects: []
                    property var commands: []
                    function registerRibbonCommand(c) { commands.push(c) }
                    function ribbonIconFor(id) { return "tree" }
                    QtObject { id: shell; property color foreground: "black"; property color background: "white"; property color accent: "blue"; property string project_dir: "/project" }
                    property string metadataFasta: currentKind === "alignment" ? "/project/rbcL.fasta" : ""
                    property var metadataLinks: currentKind === "alignment" ? ({records: 3, unlinked: ["a"]}) : ({})
                    property string submissionAlignment: currentKind === "alignment" ? "/project/rbcL.afa" : ""
                    QtObject { id: submissions; property bool available: currentKind === "alignment" }
                    QtObject { id: submissionJobs; property bool busy: false }
                    ${host.source.slice(tabsStart, tabsEnd)}
                    ${declarations}
                    ${host.block("function disabledReasonFor(")}
                    ${components}
                    ${ribbon}
                }`, host, Qt.resolvedUrl("stableRibbonHarness.qml"))
        }
        function test_disabled_prerequisite_messages() {
            const scene = productionRibbon()
            scene.currentKind = "alignment"
            scene.current = {busy: true, gap_column_count: 0, zoom_text: "100 %"}
            verify(scene.disabledReasonFor("species.gmyc-workspace").includes("Wait"))
            scene.current = {busy: false, unresolved: 2, review_mode: true, gap_column_count: 0, zoom_text: "100 %", calling: false, call_sets: "[]", selected_call_set: ""}
            scene.currentKind = "trace"
            scene.traceDocument = {working: false, traceRows: [{}], returnAlignment: null}
            verify(scene.disabledReasonFor("ribbon.export-the-edited-consensus-as-fasta").includes("Resolve"))
            verify(scene.disabledReasonFor("trace.cancel-calls").includes("Compute"))
            scene.destroy()
        }
        function test_production_pages_and_applicability_data() {
            return [
                {tag: "wide", width: 1600, factor: 1},
                {tag: "narrow", width: 320, factor: 1},
                {tag: "scaled", width: 640, factor: 1.5},
                {tag: "large", width: 800, factor: 2}
            ]
        }
        function test_production_pages_and_applicability(data) {
            const scene = productionRibbon()
            verify(scene)
            const ribbon = findChild(scene, "applicationRibbon")
            ribbon.width = data.width
            scene.ribbonSizeFactor = data.factor
            compare(scene.ribbonTabs, ["File", "View", "Annotations", "Alignment", "Phylogeny", "Tree", "Delimitation", "Trace", "Data submission", "Help"])
            scene.ribbonTabName = "Phylogeny"
            for (const kind of ["", "sequence", "alignment", "tree", "delimitation", "trace"]) {
                scene.currentKind = kind
                scene.traceDocument = kind === "trace" ? {working: false, traceRows: [{}], returnAlignment: null} : null
                scene.current = kind ? {busy: false, dirty: false, gap_column_count: 2, tip_font: 13, support_font: 10, has_support: true, linked_alignment: "", zoom_text: "100 %", bootstrap_running: false, review_mode: true, unresolved: 0, calling: false, call_sets: "[]", selected_call_set: ""} : null
                compare(scene.ribbonTab, "Phylogeny", "document switch retains tab")
                for (const name of scene.ribbonTabs) {
                    scene.ribbonTabName = name
                    verify(waitForPolish(scene.Window.window))
                    const height = ribbon.height
                    for (const command of scene.commands) {
                        if (name === "Annotations" && command.commandId === "ribbon.annotate-the-selection") compare(command.enabled, kind === "sequence")
                        if (name === "Alignment" && command.commandId === "ribbon.move-the-selected-rows-up") compare(command.enabled, kind === "alignment")
                        if (name === "Phylogeny" && command.commandId === "analysis.distance.nj") compare(command.enabled, kind === "alignment")
                        if (name === "Tree" && command.commandId === "ribbon.add-support-values-from-another-analysis-of-the-same") compare(command.enabled, kind === "tree")
                        if (name === "Delimitation" && command.commandId === "species.gmyc-workspace") compare(command.enabled, ["alignment", "tree", "delimitation"].includes(kind))
                        if (name === "Delimitation" && command.commandId === "species.asap-workspace") compare(command.enabled, kind === "alignment")
                        if (name === "Trace" && command.commandId === "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft") compare(command.enabled, kind === "trace")
                        if (name === "Data submission" && command.commandId === "metadata.table") verify(command.enabled)
                        if (name === "Data submission" && command.commandId === "metadata.import-headers") compare(command.enabled, kind === "alignment")
                        if (name === "Data submission" && command.commandId === "metadata.template") compare(command.enabled, kind === "alignment")
                        if (name === "Data submission" && command.commandId === "submission.new") compare(command.enabled, kind === "alignment")
                        if (name === "Data submission" && command.commandId === "submission.open") verify(command.enabled)
                        if (name === "Data submission" && command.commandId === "submission.validate") compare(command.enabled, kind === "alignment")
                    }
                    scene.ribbonTabName = "View"
                    verify(waitForPolish(scene.Window.window))
                    compare(ribbon.height, height, "View has the same height as " + name)
                }
                scene.ribbonTabName = "Phylogeny"
            }
            scene.destroy()
        }
    }
}
