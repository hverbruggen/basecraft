import QtQuick
import QtQuick.Controls.Basic
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
        name: "FigureExport"
        when: windowShown
        function initTestCase() {
            const request = new XMLHttpRequest()
            request.open("GET", Qt.resolvedUrl("../../qml/Main.qml"), false)
            request.send()
            host.source = request.responseText
            verify(host.source.length > 0)
        }
        function harness() {
            // Exercise production handlers/options; substitute only the native
            // picker so this test cannot open an uncontained child window.
            const picker = host.block("    FileDialog {\n        id: svgDialog").replace('FileDialog {', 'QtObject { signal accepted(); property bool visible: false; property url selectedFile; property url currentFolder; property string title; property int fileMode; property var nameFilters; function open() { visible = true }')
                .replace("fileMode: FileDialog.SaveFile", "fileMode: 0")
            return Qt.createQmlObject(`
                import QtQuick
                import QtQuick.Controls.Basic
                import QtQuick.Layouts
                import "../../qml"
                Item {
                    id: window
                    width: 800; height: 600
                    property bool treeOpen: false
                    property bool alignmentOpen: false
                    property string currentPath: "/original.nwk"
                    property var calls: []
                    property var current: ({export_figure_selection: function(path, scope, publication, selection) { window.calls.push(["svg", path, scope, publication, selection]) }, export_figure_png_selection: function(path, scope, publication, width, selection) { window.calls.push(["png", path, scope, publication, width, selection]) }, export_figure_png: function(path, scope, publication, width) { window.calls.push(["png", path, scope, publication, width]) }, export_figure: function(path, scope, publication) { window.calls.push(["svg", path, scope, publication]) }})
                    property alias options: svgOptions
                    property alias picker: svgDialog
                    property alias region: svgScope.currentIndex
                    property alias publication: svgPublication.checked
                    function activeFolder() { return "file:///" }
                    function localPath(url) { return url.toString().replace("file://", "") }
                    QtObject {
                        id: shell
                        function local_file_url(path) { return "file://" + path }
                        function local_file_path(url) { return url.toString().replace("file://", "") }
                    }
                    ${host.block("function exportSvg(")}
                    ${host.block("    AppContentDialog {\n        id: svgOptions")}
                    ${picker}
                }`, host, Qt.resolvedUrl("figureExportHarness.qml"))
        }
        function test_width_keyboard_and_format_visibility() {
            const scene = harness()
            scene.exportSvg("png")
            tryCompare(scene.options, "opened", true)
            const width = findChild(scene, "pngWidth")
            verify(width.visible)
            compare(width.value, 1500)
            compare(width.Accessible.name, "PNG width (pixels)")
            width.forceActiveFocus()
            keyClick(Qt.Key_Up)
            compare(width.value, 1501)
            width.contentItem.forceActiveFocus()
            keyClick(Qt.Key_A, Qt.ControlModifier)
            keyClick(Qt.Key_3)
            keyClick(Qt.Key_2)
            keyClick(Qt.Key_0)
            keyClick(Qt.Key_Tab)
            compare(width.value, 320)
            scene.options.reject()
            scene.exportSvg("svg")
            verify(!width.visible)
            scene.options.reject()
            scene.destroy()
        }
        function test_alignment_selection_is_optional_and_captured() {
            for (const format of ["svg", "png"]) {
                const scene = harness()
                scene.alignmentOpen = true
                scene.exportSvg(format)
                tryCompare(scene.options, "opened", true)
                const selection = findChild(scene, "svgSelection")
                verify(selection.visible)
                verify(!selection.checked)
                selection.checked = true
                scene.options.accept()
                // Switching documents/options while the picker is open must
                // retain the original export target and highlighting choice.
                scene.alignmentOpen = false
                selection.checked = false
                scene.picker.accepted()
                compare(scene.calls, [format === "png"
                    ? ["png", "/original.png", 0, false, 1500, true]
                    : ["svg", "/original.svg", 0, false, true]])
                scene.picker.visible = false
                scene.exportSvg(format)
                verify(!selection.checked)
                verify(!selection.visible)
                scene.options.reject()
                scene.destroy()
            }
        }
        function test_export_captures_controller_and_options() {
            const scene = harness()
            scene.treeOpen = true
            scene.exportSvg("png")
            tryCompare(scene.options, "opened", true)
            scene.publication = true
            findChild(scene, "pngWidth").value = 320
            scene.currentPath = "/other.nwk"
            scene.current = ({})
            scene.options.accept()
            compare(scene.picker.selectedFile.toString(), "file:///original.png")
            // Later option changes must not change a pending export.
            scene.region = 0
            scene.publication = false
            findChild(scene, "pngWidth").value = 1500
            scene.options.format = "svg"
            scene.picker.accepted()
            compare(scene.calls, [["png", "/original.png", 2, true, 320]])
            scene.picker.visible = false
            scene.destroy()
        }
    }
}
