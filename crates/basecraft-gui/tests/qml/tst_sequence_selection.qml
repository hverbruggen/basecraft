import QtQuick
import QtQuick.Controls.Basic
import QtTest

Item {
    id: host
    width: 700; height: 500
    function productionBlock(file, start) {
        const request = new XMLHttpRequest()
        request.open("GET", Qt.resolvedUrl("../../qml/" + file), false)
        request.send()
        const source = request.responseText
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
        name: "SequenceSelectionRouting"
        when: windowShown
        function test_canvas_routes_select_all_but_text_entry_handles_its_own() {
            const handler = host.productionBlock("DocumentView.qml", "Keys.onPressed:")
            const canvas = Qt.createQmlObject(`
                import QtQuick
                import QtQuick.Controls.Basic
                Item {
                    id: view; width: 600; height: 400
                    property bool renderingAvailable: true
                    property int forwardedKey: 0
                    property int forwardedModifiers: 0
                    property alias field: field
                    QtObject {
                        id: ctl
                        function key(key, text, modifiers) {
                            if (key !== Qt.Key_A) return false
                            view.forwardedKey = key; view.forwardedModifiers = modifiers; return true
                        }
                    }
                    TextField { id: field; text: "entry text" }
                    ${handler}
                }
            `, host)
            try {
                canvas.forceActiveFocus()
                keyClick(Qt.Key_A, Qt.ControlModifier)
                compare(canvas.forwardedKey, Qt.Key_A)
                compare(canvas.forwardedModifiers, Qt.ControlModifier)
                canvas.forwardedKey = 0
                canvas.field.forceActiveFocus()
                keyClick(Qt.Key_A, Qt.ControlModifier)
                compare(canvas.field.selectedText, "entry text")
                compare(canvas.forwardedKey, 0)
            } finally { canvas.destroy() }
        }
        function test_blast_defaults_selection_only_for_whole_sequence_entry() {
            const beginBlast = host.productionBlock("Main.qml", "function beginBlast(")
            const window = Qt.createQmlObject(`
                import QtQuick
                Item {
                    id: window
                    property bool alignmentOpen: false
                    property bool selection: true
                    property string shownMode: ""
                    property var current: controller
                    QtObject {
                        id: controller
                        function has_selection() { return window.selection }
                        function selected_feature() { return 1 }
                    }
                    QtObject {
                        id: blastDialog
                        function show(provider, mode, alignment) { window.shownMode = mode }
                    }
                    ${beginBlast}
                }
            `, host)
            try {
                window.beginBlast("whole"); compare(window.shownMode, "selection")
                window.selection = false
                window.beginBlast("whole"); compare(window.shownMode, "whole")
                window.selection = true
                window.beginBlast("annotation", 3); compare(window.shownMode, "annotation")
                window.alignmentOpen = true
                window.beginBlast("all"); compare(window.shownMode, "all")
            } finally { window.destroy() }
        }
    }
}
