import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 1000
    height: 900
    QtObject {
        id: first
        property string revision: "session:0:1:4"
        property string saved: ""
        property string message: ""
        property string message_kind: ""
        property int removed: 0
        property string tip: "A"
        property bool tip_selected: true
        property string renamed: ""
        property string renameError: ""
        property var current: ({id: 2, exists: false, tips: 2, clade: ["A", "B"], label: "",
            on_branch: false, bar: true, bar_width: 3, field: false, colour: "#808080", opacity_left: 30, opacity_right: 30})
        function annotation_json() { return JSON.stringify(current) }
        function record_revision() { return revision }
        function set_annotation(value) { saved = value; return "" }
        function remove_annotation(id) { removed = id }
        function tip_name() { return tip }
        function rename_tip(name) { if (renameError.length) return renameError; renamed = name; return "" }
    }
    QtObject {
        id: second
        property string saved: ""
        function record_revision() { return "other:0:1:4" }
        function set_annotation(value) { saved = value; return "" }
    }
    TextField { id: invoker; y: 850; text: "Original focus" }
    CladeEditor { id: editor; controller: first }
    TreeTipNameDialog { id: tipEditor; controller: first }
    TreeContextMenu {
        id: menu
        onNameRequested: (target, annotation, revision) => editor.openCaptured(target, annotation, revision, true)
    }
    SignalSpy { id: renameSpy; target: menu; signalName: "renameTipRequested" }
    TestCase {
        name: "TreeAnnotations"
        when: windowShown
        function init() {
            editor.close()
            tipEditor.close()
            menu.close()
            editor.controller = first
            first.revision = "session:0:1:4"
            first.saved = ""
            second.saved = ""
            first.removed = 0
            first.tip = "A"
            first.renamed = ""
            first.renameError = ""
            first.current = {id: 2, exists: false, tips: 2, clade: ["A", "B"], label: "",
                on_branch: false, bar: true, bar_width: 3, field: false, colour: "#808080", opacity_left: 30, opacity_right: 30}
            renameSpy.clear()
        }
        function cleanup() {
            editor.close()
            tipEditor.close()
            menu.close()
            tryCompare(editor, "visible", false)
        }
        function field(name) { return findChild(editor, name) }
        function test_context_name_captures_clade_and_controller() {
            menu.showFor(first, {node: true, tip: false, annotated: false})
            tryCompare(menu, "opened", true)
            const name = findChild(menu, "nameAnnotation")
            compare(name.text, "Name branch / node…")
            verify(!findChild(menu, "renameTipIdentity").visible)
            // Simulate selection and active-document changes after the popup.
            first.current = Object.assign({}, first.current, {clade: ["C", "D"], label: "other"})
            editor.controller = second
            name.triggered()
            menu.close()
            tryCompare(editor, "opened", true)
            compare(field("label").text, "")
            verify(field("onBranch").checked)
            verify(!field("bar").checked)
            field("label").text = "95 / named clade"
            editor.save()
            const saved = JSON.parse(first.saved)
            compare(saved.clade, ["A", "B"])
            compare(saved.label, "95 / named clade")
            verify(saved.on_branch)
            verify(!saved.bar)
            compare(second.saved, "")
        }
        function test_existing_name_preserves_appearance_and_cancel() {
            first.current = Object.assign({}, first.current, {exists: true, label: "Old name", field: true,
                on_branch: false, bar: true, colour: "#2980b9", opacity_left: 12})
            invoker.forceActiveFocus()
            editor.openCaptured(first, first.current, first.revision, true)
            tryCompare(editor, "opened", true)
            compare(field("label").text, "Old name")
            verify(!field("onBranch").checked)
            verify(field("bar").checked)
            compare(field("label").Accessible.name, "Name")
            tryVerify(() => field("label").activeFocus)
            field("label").text = "Discard me"
            keyClick(Qt.Key_Escape)
            tryCompare(editor, "visible", false)
            compare(first.saved, "")
            compare(first.removed, 0)
            tryVerify(() => invoker.activeFocus)
            editor.openSelected()
            tryCompare(editor, "opened", true)
            editor.save()
            const saved = JSON.parse(first.saved)
            compare(saved.label, "Old name")
            compare(saved.colour, "#2980b9")
            compare(saved.opacity_left, 12)
            verify(saved.field && saved.bar && !saved.on_branch)
        }
        function test_stale_document_is_rejected() {
            editor.openSelected()
            tryCompare(editor, "opened", true)
            first.revision = "session:0:2:5"
            field("label").text = "Late name"
            editor.save()
            compare(first.saved, "")
            verify(editor.visible)
            verify(field("annotationError").text.includes("tree changed"))
        }
        function test_tip_annotation_and_identity_are_distinct() {
            first.current = Object.assign({}, first.current, {clade: ["A"], tips: 1})
            menu.showFor(first, {node: true, tip: true, annotated: false})
            tryCompare(menu, "opened", true)
            compare(findChild(menu, "nameAnnotation").text, "Name tip annotation…")
            const rename = findChild(menu, "renameTipIdentity")
            compare(rename.text, "Rename tip identity…")
            verify(rename.visible)
            rename.triggered()
            compare(renameSpy.count, 1)
            compare(renameSpy.signalArguments[0][0], first)
            compare(renameSpy.signalArguments[0][1], "A")
            compare(renameSpy.signalArguments[0][2], first.revision)
            compare(first.saved, "")
        }
        function test_stale_context_is_rejected() {
            menu.showFor(first, {node: true, tip: false, annotated: false})
            tryCompare(menu, "opened", true)
            first.revision = "session:0:1:5"
            findChild(menu, "nameAnnotation").triggered()
            verify(!editor.visible)
            verify(first.message.includes("tree changed"))
            compare(first.saved, "")
        }
        function test_tip_escape_discards_edit_and_restores_focus() {
            invoker.forceActiveFocus()
            tipEditor.ask()
            tryCompare(tipEditor, "opened", true)
            const name = findChild(tipEditor, "tipName")
            tryVerify(() => name.activeFocus)
            compare(name.text, "A")
            name.text = "Discard this"
            keyClick(Qt.Key_Escape)
            tryCompare(tipEditor, "visible", false)
            compare(first.renamed, "")
            tryVerify(() => invoker.activeFocus)
        }
        function test_tip_target_and_errors() {
            tipEditor.ask()
            tryCompare(tipEditor, "opened", true)
            const name = findChild(tipEditor, "tipName")
            name.text = "Bee"
            first.tip = "B"
            tipEditor.save()
            compare(first.renamed, "")
            verify(findChild(tipEditor, "tipNameError").text.includes("selected tip changed"))
            first.tip = "A"
            first.renameError = "there's already a tip named Bee"
            tipEditor.save()
            verify(tipEditor.visible)
            compare(first.renamed, "")
            compare(findChild(tipEditor, "tipNameError").text, first.renameError)
            first.renameError = ""
            tipEditor.controller = second
            tipEditor.save()
            compare(first.renamed, "Bee", "active tab cannot redirect the rename")
            compare(second.saved, "")
            tryCompare(tipEditor, "visible", false)
        }
    }
}
