import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 800
    height: 600
    CladeEditor { id: clade }
    TreeStyleDialog { id: style }
    AppErrorLabel { id: error; text: "Invalid location: enter a valid GenBank range." }
    TestCase {
        name: "DialogAccessibility"
        when: windowShown
        function checkField(dialog, objectName, label, name) {
            const field = findChild(dialog.contentItem, objectName)
            verify(field !== null, objectName)
            compare(field.Accessible.labelledBy.text, label)
            compare(field.Accessible.name, name || label)
        }
        function test_clade_fields_follow_visible_labels() {
            clade.open()
            tryCompare(clade, "opened", true)
            checkField(clade, "label", "Name")
            checkField(clade, "barWidth", "Thickness")
            checkField(clade, "colour", "Colour")
            checkField(clade, "left", "Field opacity at left", "Field opacity at left (%)")
            checkField(clade, "right", "Field opacity at right", "Field opacity at right (%)")
            clade.close()
        }
        function test_tree_fields_follow_visible_labels() {
            style.open()
            tryCompare(style, "opened", true)
            checkField(style, "minimum", "Hide values below", "Hide values below (%)")
            checkField(style, "symbolSet", "Circles at nodes for")
            checkField(style, "tipFont", "Tip names")
            checkField(style, "supportFont", "Support values")
            checkField(style, "branchWidth", "Branches")
            style.close()
        }
        function test_errors_expose_role_and_current_message() {
            compare(error.Accessible.role, Accessible.AlertMessage)
            compare(error.Accessible.name, error.text)
            error.text = "The destination is read-only. Choose another folder."
            compare(error.Accessible.name, error.text)
        }
    }
}
