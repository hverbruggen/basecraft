import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: root
    width: 800; height: 640
    QtObject {
        id: controller
        property var saved: null
        function style_json() {
            return JSON.stringify({
                sets: [{name: "Bootstrap", source: "", decimals: 0}, {name: "BPP", source: "", decimals: 2}],
                style: {support_shown: [], support_min: 0, support_decimals: {}, support_proportions: {},
                    symbols: null, tip_font: 13, support_font: 10, branch_width: 1,
                    vertical_bar_labels: true, scale_bar: true, future_option: {value: 42}}
            })
        }
        function set_style(value) { saved = JSON.parse(value); return "" }
    }
    TreeStyleDialog { id: dialog; controller: controller }
    TestCase {
        name: "TreeStyle"
        when: windowShown
        function init() { controller.saved = null; dialog.openStyle(); tryCompare(dialog, "opened", true); wait(20) }
        function cleanup() { dialog.close(); tryCompare(dialog, "opened", false) }
        function test_independent_precision_keyboard_and_unknown_settings() {
            const auto = findChild(dialog.contentItem, "autoDecimals0")
            const bootstrap = findChild(dialog.contentItem, "supportDecimals0")
            const bpp = findChild(dialog.contentItem, "supportDecimals1")
            compare(bootstrap.value, 0)
            compare(bpp.value, 2)
            compare(bootstrap.Accessible.name, "Bootstrap decimal places")
            verify(!bootstrap.enabled)
            auto.forceActiveFocus()
            tryCompare(auto, "activeFocus", true)
            keyClick(Qt.Key_Space)
            verify(bootstrap.enabled)
            bootstrap.forceActiveFocus()
            keyClick(Qt.Key_Up)
            compare(bootstrap.value, 1)
            verify(bootstrap.width >= bootstrap.implicitWidth)
            dialog.save()
            compare(controller.saved.support_decimals.Bootstrap, 1)
            verify(!Object.prototype.hasOwnProperty.call(controller.saved.support_decimals, "BPP"))
            compare(controller.saved.future_option.value, 42)
        }
        function test_escape_discards_pending_changes() {
            const auto = findChild(dialog.contentItem, "autoDecimals0")
            auto.forceActiveFocus()
            tryCompare(auto, "activeFocus", true)
            keyClick(Qt.Key_Space)
            const bootstrap = findChild(dialog.contentItem, "supportDecimals0")
            bootstrap.forceActiveFocus()
            keyClick(Qt.Key_Up)
            compare(bootstrap.value, 1)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            compare(controller.saved, null)
            dialog.openStyle()
            tryCompare(dialog, "opened", true)
            compare(findChild(dialog.contentItem, "supportDecimals0").value, 0)
        }
        function test_auto_preserves_semantic_defaults() {
            dialog.save()
            compare(Object.keys(controller.saved.support_decimals).length, 0)
            compare(Object.keys(controller.saved.support_proportions).length, 0)
        }
    }
}
