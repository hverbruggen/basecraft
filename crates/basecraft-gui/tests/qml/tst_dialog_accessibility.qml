import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 800
    height: 600
    CladeEditor { id: clade }
    QtObject {
        id: treeController
        function style_json() {
            return JSON.stringify({
                sets: [{name: "Bootstrap", source: "", decimals: 0, automatic_decimals: 0, proportions: false}],
                style: {support_shown: [], support_min: 0, support_decimals: {}, support_proportions: {},
                    symbols: null, branch_width: 1, vertical_bar_labels: true, scale_bar: true}
            })
        }
    }
    TreeStyleDialog { id: style; controller: treeController }
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
            checkField(clade, "colorHex", "Colour")
            checkField(clade, "left", "Field opacity at left", "Field opacity at left (%)")
            checkField(clade, "right", "Field opacity at right", "Field opacity at right (%)")
            clade.close()
        }
        function test_tree_fields_follow_visible_labels() {
            style.openStyle("support")
            tryCompare(style, "opened", true)
            checkField(style, "supportMinimum0", "Hide below (%)", "Bootstrap hide below (percent)")
            checkField(style, "symbolSet", "Circles at nodes for")
            style.close()
            tryCompare(style, "opened", false)
            style.openStyle("extra")
            tryCompare(style, "opened", true)
            checkField(style, "branchWidth", "Branch width (px)")
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
