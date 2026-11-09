import QtQuick
import QtTest
import "../../qml"

Item {
    id: host
    width: 800; height: 650
    property string source
    function declaration(id) {
        const begin = source.lastIndexOf("AppCommand {", source.indexOf("id: " + id))
        let end = source.indexOf("{", begin) + 1, depth = 1
        while (depth && end < source.length) {
            const c = source[end++]
            if (c === "{") depth++
            if (c === "}") depth--
        }
        return source.slice(begin, end)
    }
    TestCase {
        name: "SequenceCommands"
        when: windowShown
        function initTestCase() {
            const request = new XMLHttpRequest()
            request.open("GET", Qt.resolvedUrl("../../qml/Main.qml"), false)
            request.send()
            host.source = request.responseText
            verify(host.source.length > 0)
        }
        function harness() {
            const item = Qt.createQmlObject(`
                import QtQuick
                import QtQuick.Controls.Basic
                import "../../qml"
                Item {
                    id: window
                    property string currentKind: "sequence"
                    property var current: ({busy: false})
                    property var customization: ({shortcuts: {}, ribbon_hidden: []})
                    property var activeFocusItem: canvas.activeFocus ? canvas : field
                    property int annotations: 0
                    property int sequences: 0
                    property alias annotate: addAnnotationCommand
                    property alias create: newSequenceCommand
                    property alias textField: field
                    property alias document: canvas
                    // No ribbon-page state: commands are independent of its visibility.
                    Item { id: canvas; width: 300; height: 200; focus: true }
                    TextField { id: field; y: 220; text: "Editable text" }
                    QtObject { id: editor; function openNew() { window.annotations++ } }
                    QtObject { id: newSequenceDialog; function prepare(text, name) { window.sequences++ } }
                    ${host.declaration("newSequenceCommand")}
                    ${host.declaration("addAnnotationCommand")}
                }`, host, "sequenceCommandHarness")
            verify(item !== null)
            return item
        }
        function test_defaults_text_context_and_remapping() {
            const h = harness()
            h.document.forceActiveFocus()
            tryCompare(h.annotate, "enabled", true)
            keyClick(Qt.Key_N, Qt.ControlModifier)
            compare(h.sequences, 1)
            compare(h.annotations, 0)
            keyClick(Qt.Key_N, Qt.ControlModifier | Qt.ShiftModifier)
            compare(h.annotations, 1)
            h.textField.forceActiveFocus()
            tryCompare(h.annotate, "enabled", false)
            keyClick(Qt.Key_N, Qt.ControlModifier | Qt.ShiftModifier)
            compare(h.annotations, 1)
            h.customization = {shortcuts: {"ribbon.annotate-the-selection": "Ctrl+J", "file.new-sequence": "Ctrl+K"}, ribbon_hidden: []}
            h.document.forceActiveFocus()
            keyClick(Qt.Key_N, Qt.ControlModifier | Qt.ShiftModifier)
            keyClick(Qt.Key_N, Qt.ControlModifier)
            compare(h.annotations, 1)
            compare(h.sequences, 1)
            keyClick(Qt.Key_J, Qt.ControlModifier)
            keyClick(Qt.Key_K, Qt.ControlModifier)
            compare(h.annotations, 2)
            compare(h.sequences, 2)
            h.currentKind = "alignment"
            tryCompare(h.annotate, "enabled", false)
            h.destroy()
        }
    }
}
