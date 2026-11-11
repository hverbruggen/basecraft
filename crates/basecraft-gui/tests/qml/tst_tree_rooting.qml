import QtQuick
import QtTest
import "../../qml"

Item {
    QtObject {
        id: captionSource
        readonly property var captions: ({"ribbon.root-the-tree-on-the-branch-above-the-selection": "Reroot", "tree.midpoint_root": "Midpoint", "tree.minimum_variance_root": "MRTTV"})
        function caption(id) { return captions[id] || "" }
        function tip(id) { return "" }
    }
    id: host
    width: 600; height: 140
    property real sizeFactor: 1
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
    Ribbon {
        width: host.width
        height: implicitHeight
        ribbonSizeFactor: host.sizeFactor
        RibbonPage { TreeRootingGroup { id: group; controller: controller; treeOpen: true; captions: captionSource } }
    }
    TestCase {
        name: "TreeRooting"
        when: windowShown
        function init() {
            host.sizeFactor = 1
            group.controller = controller
            group.treeOpen = true
            controller.has_selection = false
            controller.can_auto_root = true
            controller.busy = false
            controller.last = ""
        }
        function test_reduced_reroot_arrow_scales() {
            host.sizeFactor = 2
            compare(findChild(group, "manualRoot").symbolSize, 16 * 0.65)
            compare(findChild(group, "manualRoot").icon.width, Math.round(32 * 0.65))
            compare(findChild(group, "midpointRoot").icon.width, 32)
        }
        function test_selection_and_routing() {
            const manual = findChild(group, "manualRoot")
            const midpoint = findChild(group, "midpointRoot")
            const variance = findChild(group, "minimumVarianceRoot")
            compare(manual.iconName, "left")
            compare(midpoint.iconName, "tree-midpoint")
            compare(variance.iconName, "tree-variance")
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
