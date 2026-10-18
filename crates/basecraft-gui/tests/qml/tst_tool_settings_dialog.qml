import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"
import "tool_forms.js" as Forms

Item {
    width: 900; height: 700
    Component { id: factory; ToolSettingsDialog {} }

    TestCase {
        name: "ToolSettingsDialog"
        when: windowShown

        // Fields come from a Repeater: search the dialog's visual content.
        function field(dialog, name) { return findChild(dialog.contentItem, name) }
        function open(plugin, id) {
            const target = Forms.target()
            const dialog = createTemporaryObject(factory, parent)
            dialog.show(target, plugin, id)
            tryCompare(dialog, "opened", true)
            return [dialog, target]
        }

        function test_iqtree_rates_apply_to_a_chosen_model_and_ufboot_needs_1000() {
            const [dialog, target] = open("iqtree", "tree")
            compare(dialog.title, "IQ-TREE")
            compare(dialog.standardButton(Dialog.Ok).text, "Build tree")
            const gamma = field(dialog, "gamma")
            verify(!gamma.enabled, "ModelFinder chooses rate variation itself")
            dialog.set("model", "GTR")
            verify(gamma.enabled)
            const replicates = field(dialog, "replicates")
            verify(!replicates.enabled)
            dialog.set("support", "bootstrap")
            dialog.set("replicates", 100)
            verify(replicates.enabled)
            compare(replicates.value, 100)
            dialog.set("support", "ufboot")
            tryCompare(replicates, "value", 1000)
            compare(dialog.values.replicates, 1000)
            dialog.accept()
            compare(target.runs.length, 1)
            compare(target.runs[0].plugin, "iqtree")
            compare(target.runs[0].values.model, "GTR")
            compare(target.runs[0].values.replicates, 1000)
        }

        function test_bootstrap_trees_use_the_sorted_method() {
            const [dialog, target] = open("distance", "nj")
            const method = field(dialog, "method")
            verify(method.enabled)
            compare(method.currentText, "njstar")
            field(dialog, "bootstrap_on").click()
            verify(!method.enabled)
            compare(method.currentText, "sorted")
            verify(field(dialog, "bootstrap").enabled)
            dialog.reject()
            compare(target.runs.length, 0)
        }

        function test_unknown_forms_report_instead_of_opening() {
            const target = Forms.target()
            const dialog = createTemporaryObject(factory, parent)
            dialog.show(target, "mafft", "tree")
            compare(dialog.opened, false)
            compare(target.message_kind, "error")
        }
    }
}
