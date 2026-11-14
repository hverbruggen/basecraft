import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

// D29: the offer to number duplicate names opens when an editor has one and
// no alphabet question is waiting. Focus starts on Cancel, so a key press
// changes nothing; the action button numbers through the editor.
Item {
    id: scene
    width: 900
    height: 600

    Component {
        id: editorComponent
        QtObject {
            property string duplicate_names: ""
            property string alphabet_question: ""
            property int numbered: 0
            function number_duplicates() { numbered++; duplicate_names = ""; return true }
        }
    }
    Component { id: dialogComponent; DuplicateNamesDialog {} }

    TestCase {
        name: "DuplicateNamesDialog"
        when: windowShown

        readonly property string offer: JSON.stringify({
            text: "Some records in two.gb share a name: ABC. Number them in the file (ABC.1, ABC.2) so each can be edited? This changes the record names in the file itself.",
            action: "&Number in file"
        })

        function offered(question) {
            const editor = createTemporaryObject(editorComponent, scene, { alphabet_question: question || "" })
            const dialog = createTemporaryObject(dialogComponent, scene, { controller: editor })
            verify(!dialog.visible, "No offer while every name is unique")
            editor.duplicate_names = offer
            return { editor: editor, dialog: dialog }
        }

        function test_focus_starts_on_cancel() {
            const { editor, dialog } = offered()
            tryCompare(dialog, "opened", true)
            tryCompare(dialog.standardButton(Dialog.Cancel), "activeFocus", true)
            keyClick(Qt.Key_Return)
            compare(editor.numbered, 0, "Enter must not change the file")
            keyClick(Qt.Key_Space)
            tryCompare(dialog, "visible", false)
            compare(editor.numbered, 0, "The focused button cancels")
            compare(editor.duplicate_names, "")
        }

        function test_number_in_file() {
            const { editor, dialog } = offered()
            tryCompare(dialog, "opened", true)
            const button = findChild(dialog.body, "numberDuplicates")
            compare(button.text, "&Number in file")
            mouseClick(button)
            compare(editor.numbered, 1)
            tryCompare(dialog, "visible", false)
        }

        function test_waits_for_the_alphabet_question() {
            const { editor, dialog } = offered("Is this DNA, RNA or protein?")
            wait(50)
            verify(!dialog.visible, "One question at a time")
            editor.alphabet_question = ""
            tryCompare(dialog, "opened", true)
        }

        function test_escape_leaves_names() {
            const { editor, dialog } = offered()
            tryCompare(dialog, "opened", true)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "visible", false)
            compare(editor.numbered, 0)
            compare(editor.duplicate_names, "")
        }
    }
}
