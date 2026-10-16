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
        property var removed: []
        function style_json() {
            return JSON.stringify({
                sets: [{name: "Bootstrap", source: "", decimals: 0, automatic_decimals: 0}, {name: "BPP", source: "", decimals: 2, automatic_decimals: 2}],
                style: {support_shown: [], support_min: 0, support_decimals: {}, support_proportions: {},
                    symbols: null, tip_font: 13, support_font: 10, branch_width: 1,
                    vertical_bar_labels: true, scale_bar: true, future_option: {value: 42}}
            })
        }
        function set_style_with_removals(value, names) { saved = JSON.parse(value); removed = JSON.parse(names); return "" }
    }
    TreeStyleDialog { id: dialog; controller: controller }
    TextField { id: invoker; text: "Document" }
    SignalSpy { id: rejected; target: dialog; signalName: "rejected" }
    SignalSpy { id: accepted; target: dialog; signalName: "accepted" }
    TestCase {
        name: "TreeStyle"
        when: windowShown
        function init() { controller.saved = null; controller.removed = []; invoker.forceActiveFocus(); rejected.clear(); accepted.clear(); dialog.openStyle(); tryCompare(dialog, "opened", true); wait(20) }
        function cleanup() { dialog.close(); tryCompare(dialog, "opened", false); root.width = 800; dialog.font.pixelSize = 13 }
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
            compare(rejected.count, 1)
            compare(accepted.count, 0)
            compare(controller.saved, null)
            tryCompare(invoker, "activeFocus", true)
            dialog.openStyle()
            tryCompare(dialog, "opened", true)
            compare(findChild(dialog.contentItem, "supportDecimals0").value, 0)
        }
        function test_escape_discards_staged_support_removal() {
            const remove = findChild(dialog.contentItem, "removeSupport0")
            remove.forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(dialog.removedSets.length, 1)
            compare(controller.saved, null)
            keyClick(Qt.Key_Escape)
            tryCompare(dialog, "opened", false)
            compare(rejected.count, 1)
            compare(accepted.count, 0)
            compare(controller.removed.length, 0)
            tryCompare(invoker, "activeFocus", true)
            dialog.openStyle()
            tryCompare(dialog, "opened", true)
            compare(dialog.removedSets.length, 0)
            compare(findChild(dialog.contentItem, "supportDecimals0").value, 0)
        }
        function test_save_applies_staged_support_removal() {
            const remove = findChild(dialog.contentItem, "removeSupport0")
            remove.forceActiveFocus()
            keyClick(Qt.Key_Space)
            dialog.save()
            compare(controller.removed.length, 1)
            compare(controller.removed[0], "Bootstrap")
        }
        function test_new_controls_fit_font_and_window_data() {
            return [{tag: "small", width: 480, fontSize: 16},
                {tag: "large-font-small", width: 480, fontSize: 24},
                {tag: "large-font-wide", width: 800, fontSize: 24}]
        }
        function test_new_controls_fit_font_and_window(data) {
            root.width = data.width
            dialog.font.pixelSize = data.fontSize
            wait(20)
            for (const name of ["autoDecimals0", "supportDecimals0", "supportScale0"]) {
                const field = findChild(dialog.contentItem, name)
                const point = field.mapToItem(dialog.contentItem, 0, 0)
                verify(point.x >= -1, name + " left edge")
                verify(point.x + field.width <= dialog.contentItem.width + 1, name + " right edge")
            }
            const precision = findChild(dialog.contentItem, "supportDecimals0")
            verify(precision.width >= precision.implicitWidth - 1)
        }
        function test_auto_preserves_semantic_defaults() {
            dialog.save()
            compare(Object.keys(controller.saved.support_decimals).length, 0)
            compare(Object.keys(controller.saved.support_proportions).length, 0)
        }
    }
}
