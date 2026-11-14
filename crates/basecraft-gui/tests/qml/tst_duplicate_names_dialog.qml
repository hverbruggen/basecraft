import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

// D29: the offer to number duplicate names opens when an editor has one,
// numbers through the editor, and Escape leaves the names as they are.
Item {
    id: scene
    width: 900
    height: 600

    Component {
        id: editorComponent
        QtObject {
            property string duplicate_names: ""
            property int numbered: 0
            function number_duplicates() { numbered++; duplicate_names = ""; return true }
        }
    }
    Component { id: dialogComponent; DuplicateNamesDialog {} }

    TestCase {
        name: "DuplicateNamesDialog"
        when: windowShown

        function offered() {
            const editor = createTemporaryObject(editorComponent, scene)
            const dialog = createTemporaryObject(dialogComponent, scene, { controller: editor })
            verify(!dialog.visible, "No offer while every name is unique")
            editor.duplicate_names = "Some rows share a name: a. Number them (a.1, a.2) so each can be edited?"
            tryCompare(dialog, "opened", true)
            return { editor: editor, dialog: dialog }
        }

        function test_number_them() {
            const { editor, dialog } = offered()
            const button = findChild(dialog.body, "numberDuplicates")
            tryCompare(button, "activeFocus", true)
            keyClick(Qt.Key_Space)
            compare(editor.numbered, 1)
            tryCompare(dialog, "visible", false)
        }

        function test_escape_leaves_names() {
            const { editor, dialog } = offered()
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "visible", false)
            compare(editor.numbered, 0)
            compare(editor.duplicate_names, "")
        }
    }
}
