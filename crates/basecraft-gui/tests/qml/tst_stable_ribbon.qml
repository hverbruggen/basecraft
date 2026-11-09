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
            const declarations = names.map(n => {
                const command = host.block("AppCommand {\n        id: " + n)
                const id = /commandId: "([^"]+)"/.exec(command)
                const text = /text: "([^"]+)"/.exec(command)
                const icon = /iconName: "([^"]+)"/.exec(command)
                return `AppCommand { id: ${n}; commandId: "${id[1]}"; text: "${text[1]}"; iconName: "${icon ? icon[1] : ""}" }`
            }).join("\n")
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
                    ${host.block("function ribbonIconFor(")}
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
        function test_contextual_selection_and_retention() {
            const scene = productionRibbon()
            compare(scene.ribbonTabs, ["File", "View", "Export"])
            scene.currentKind = "sequence"
            scene.ribbonTabName = "Sequence"
            scene.currentKind = "alignment"
            compare(scene.ribbonTabName, "Alignment")
            compare(scene.ribbonTabs, ["File", "View", "Alignment", "Phylogeny", "Delimitation", "Export"])
            scene.ribbonTabName = "Export"
            scene.currentKind = "tree"
            compare(scene.ribbonTabName, "Export")
            scene.ribbonTabName = "Tree"
            scene.currentKind = ""
            compare(scene.ribbonTabName, "File")
            scene.destroy()
        }
        function test_ribbon_visual_evidence() {
            const args = Qt.application.arguments
            const index = args.indexOf("--evidence-dir")
            if (index < 0) return
            const destination = args[index+1]
            const scene = productionRibbon()
            const ribbon = findChild(scene, "applicationRibbon")
            scene.currentKind = "trace"
            scene.traceDocument = {working:false, traceRows:[{}], returnAlignment:null}
            scene.current = {busy:false, review_mode:true, unresolved:0, calling:false, call_sets:"[]", selected_call_set:"", zoom_text:"100 %"}
            scene.ribbonTabName = "Trace"
            for (const factor of [1, 1.5]) {
                scene.ribbonSizeFactor = factor
                ribbon.width = 1200
                for (const dark of [false,true]) {
                    ribbon.palette.buttonText = dark ? "#eeeeee" : "#222222"
                    ribbon.palette.window = dark ? "#222222" : "#ffffff"
                    verify(waitForPolish(scene.Window.window))
                    wait(20)
                    const file = destination + "/trace-" + factor + "-" + (dark ? "dark" : "light") + ".png"
                    verify(grabImage(ribbon).save(file))
                    console.log("RIBBON_EVIDENCE", file, ribbon.width, ribbon.height)
                }
            }
            scene.destroy()
        }
        function test_export_tab_reuses_actions_and_retains_submission() {
            const scene = productionRibbon()
            scene.ribbonTabName = "Export"
            findChild(scene, "applicationRibbon").width = 1200
            verify(waitForPolish(scene.Window.window))
            function captioned(item, text) {
                if (item.text === text && item.action !== undefined) return item
                for (const child of item.children || []) {
                    const found = captioned(child, text)
                    if (found) return found
                }
                return null
            }
            const svg = captioned(scene, "SVG")
            const png = captioned(scene, "PNG")
            verify(svg && png)
            compare(svg.action.commandId, "ribbon.export-an-editable-svg-of-this-view")
            compare(png.action.commandId, "figure.export-png")
            let svgCount = 0, pngCount = 0
            svg.action.triggered.connect(() => svgCount++)
            png.action.triggered.connect(() => pngCount++)
            mouseClick(svg)
            png.forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(svgCount, 1)
            compare(pngCount, 1)
            verify(findChild(scene, "ribbonFigureExport"), "Utility export remains present")
            function containsCommand(item, id) {
                return item.commandId === id || (item.children || []).some(child => containsCommand(child, id))
            }
            const page = findChild(scene, "applicationRibbon").currentPage
            for (const id of ["metadata.table", "metadata.import-headers", "metadata.import-sheet", "metadata.template", "submission.new", "submission.open", "submission.prepare", "submission.validate", "submission.submitter"])
                verify(containsCommand(page, id), "Export retains " + id)
            scene.destroy()
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
            compare(scene.ribbonTabs, ["File", "View", "Export"])
            scene.ribbonTabName = "Phylogeny"
            for (const kind of ["", "sequence", "alignment", "tree", "delimitation", "trace"]) {
                scene.currentKind = kind
                scene.traceDocument = kind === "trace" ? {working: false, traceRows: [{}], returnAlignment: null} : null
                scene.current = kind ? {busy: false, dirty: false, gap_column_count: 2, tip_font: 13, support_font: 10, has_support: true, linked_alignment: "", zoom_text: "100 %", bootstrap_running: false, review_mode: true, unresolved: 0, calling: false, call_sets: "[]", selected_call_set: ""} : null
                verify(scene.ribbonTabs.includes(scene.ribbonTab))
                for (const name of scene.ribbonTabs) {
                    scene.ribbonTabName = name
                    verify(waitForPolish(scene.Window.window))
                    const height = ribbon.height
                    verify(!ribbon.overflowing, name + " fits without horizontal scrolling: " + ribbon.pageWidth + "/" + ribbon.availableWidth + " " + JSON.stringify(ribbon.groups().map(g=>({title:g.title, mode:g.presentation, width:g.implicitWidth}))))
                    for (const command of scene.commands) {
                        if (name === "Sequence" && command.commandId === "ribbon.annotate-the-selection") compare(command.enabled, kind === "sequence")
                        if (name === "Alignment" && command.commandId === "ribbon.move-the-selected-rows-up") compare(command.enabled, kind === "alignment")
                        if (name === "Phylogeny" && command.commandId === "analysis.distance.nj") compare(command.enabled, kind === "alignment")
                        if (name === "Tree" && command.commandId === "ribbon.add-support-values-from-another-analysis-of-the-same") compare(command.enabled, kind === "tree")
                        if (name === "Delimitation" && command.commandId === "species.gmyc-workspace") compare(command.enabled, ["alignment", "tree", "delimitation"].includes(kind))
                        if (name === "Delimitation" && command.commandId === "species.asap-workspace") compare(command.enabled, kind === "alignment")
                        if (name === "Trace" && command.commandId === "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft") compare(command.enabled, kind === "trace")
                        if (name === "Export" && command.commandId === "metadata.table") verify(command.enabled)
                        if (name === "Export" && command.commandId === "metadata.import-headers") compare(command.enabled, kind === "alignment")
                        if (name === "Export" && command.commandId === "metadata.template") compare(command.enabled, kind === "alignment")
                        if (name === "Export" && command.commandId === "submission.new") compare(command.enabled, kind === "alignment")
                        if (name === "Export" && command.commandId === "submission.open") verify(command.enabled)
                        if (name === "Export" && command.commandId === "submission.validate") compare(command.enabled, kind === "alignment")
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
