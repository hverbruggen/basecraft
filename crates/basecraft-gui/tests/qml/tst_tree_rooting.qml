import QtQuick
import QtTest
import "../../qml"

Item {
    width: 600; height: 140
    QtObject {
        id: controller
        property bool has_selection: false
        property bool can_auto_root: true
        property bool busy: false
        property string last: ""
        function reroot() { last = "manual" }
        function midpoint_root() { last = "midpoint" }
        function minimum_variance_root() { last = "variance" }
    }
    TreeRootingGroup { id: group; controller: controller; treeOpen: true }
    TestCase {
        name: "TreeRooting"
        when: windowShown
        function init() {
            group.controller = controller
            group.treeOpen = true
            controller.has_selection = false
            controller.can_auto_root = true
            controller.busy = false
            controller.last = ""
        }
        function test_selection_and_routing() {
            const manual = findChild(group, "manualRoot")
            const midpoint = findChild(group, "midpointRoot")
            const variance = findChild(group, "minimumVarianceRoot")
            compare(manual.iconName, "left")
            compare(midpoint.iconName, "tree")
            compare(variance.iconName, "tree")
            compare(midpoint.text, "Midpoint")
            compare(variance.text, "MRTTV")
            verify(!manual.enabled)
            verify(midpoint.enabled)
            verify(variance.enabled)
            midpoint.commandAction.trigger()
            compare(controller.last, "midpoint")
            variance.commandAction.trigger()
            compare(controller.last, "variance")
            controller.has_selection = true
            verify(manual.enabled)
            manual.commandAction.trigger()
            compare(controller.last, "manual")
        }
        function test_compatible_document_and_busy() {
            const midpoint = findChild(group, "midpointRoot")
            const variance = findChild(group, "minimumVarianceRoot")
            controller.busy = true
            verify(!midpoint.enabled && !variance.enabled)
            controller.busy = false
            controller.can_auto_root = false
            verify(!midpoint.enabled && !variance.enabled)
            controller.can_auto_root = true
            group.treeOpen = false
            verify(!midpoint.enabled && !variance.enabled)
            group.treeOpen = true
            group.controller = null
            verify(!midpoint.enabled && !variance.enabled)
        }
    }
}
