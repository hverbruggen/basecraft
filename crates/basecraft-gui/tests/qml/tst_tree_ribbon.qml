import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"

Item {
    id: host
    width: 1200
    height: 300
    property int textSize: 12
    property int annotations: 0
    property var linkTarget: null
    QtObject {
        id: controller
        property bool has_selection: false
        property bool busy: false
        property bool can_auto_root: true
        property string linked_alignment: ""
        property string alignment_link_status: ""
        property string source_alignment: ""
        property string last: ""
        function rotate() { last = "flip" }
        property int tip_font: 13
        function ladderize(small) { last = small ? "small" : "large" }
        function set_sizes(tip, support, width) { last = "sizes " + tip + " " + support + " " + width }
        function extract_clade_alignment() { last = "extract" }
    }
    Ribbon {
        id: ribbon
        width: 1000
        height: implicitHeight
        RibbonPage {
            TreeRootingGroup { id: root; controller: controller; treeOpen: true }
            TreeCladeGroup { id: clade; controller: controller; onAnnotateRequested: host.annotations++ }
            TreeStyleGroup { id: order; controller: controller }
            TreeAlignmentGroup { id: alignment; controller: controller; onLinkRequested: target => host.linkTarget = target }
        }
    }
    TestCase {
        name: "TreeRibbon"
        when: windowShown
        function buttons() {
            return ["manualRoot", "midpointRoot", "minimumVarianceRoot", "annotateClade", "flipSubtree", "orderBranches", "treeStyleExtra", "linkTreeAlignment", "openCladeAlignment"]
                .map(name => findChild(ribbon, name))
        }
        function init() {
            controller.has_selection = true
            controller.last = ""
            host.annotations = 0
            host.linkTarget = null
            controller.busy = false
            controller.linked_alignment = ""
            controller.alignment_link_status = ""
            controller.source_alignment = ""
            clade.controller = controller
            order.controller = controller
            ribbon.width = 1000
            for (const button of buttons()) button.font.pixelSize = 12
            failOnWarning(/Binding loop/)
            verify(waitForPolish(ribbon.Window.window))
        }
        function test_actions_and_icons() {
            compare(clade.commands.length, 2, "Root position and Rename are absent from the clade ribbon")
            compare(order.title, "Tree style")
            const flip = findChild(clade, "flipSubtree")
            compare(flip.iconName, "tree-flip")
            compare(findChild(clade, "annotateClade").iconName, "clade-annotate")
            compare(flip.Accessible.name, "Flip")
            flip.commandAction.trigger()
            compare(controller.last, "flip")
            findChild(clade, "annotateClade").commandAction.trigger()
            compare(host.annotations, 1)
            // Order is one button with a menu; its two commands stay available.
            compare(findChild(order, "orderBranches").iconName, "tree-small-on-top")
            const small = order.commands[1]
            const large = order.commands[2]
            compare(small.iconName, "tree-small-on-top")
            compare(large.iconName, "tree-large-on-top")
            small.trigger()
            compare(controller.last, "small")
            large.trigger()
            compare(controller.last, "large")
            findChild(order, "tipFont").edited(15)
            compare(controller.last, "sizes 15 0 0")
            controller.last = "large"
            controller.has_selection = false
            verify(!flip.enabled)
            flip.commandAction.trigger()
            compare(controller.last, "large")
            clade.controller = null
            order.controller = null
            verify(!small.enabled && !large.enabled)
        }
        function test_caption_alignment_data() {
            const rows = []
            for (const width of [240, 640, 1000])
                for (const font of [10, 15, 22])
                    rows.push({ tag: `${width}-${font}`, width, font })
            return rows
        }
        function test_link_status_change_and_routing() {
            const link = findChild(alignment, "linkTreeAlignment")
            const extract = findChild(alignment, "openCladeAlignment")
            compare(link.text, "Link alignment")
            compare(link.iconName, "align")
            link.commandAction.trigger()
            compare(host.linkTarget, controller)
            controller.linked_alignment = "/view.afa"
            controller.source_alignment = "/original.afa"
            controller.alignment_link_status = "Linked to retained input snapshot"
            compare(link.text, "Change alignment")
            verify(link.tip.includes("/view.afa"))
            verify(link.tip.includes("Original analysis source: /original.afa"))
            verify(link.tip.includes("retained input snapshot"))
            extract.commandAction.trigger()
            compare(controller.last, "extract")
            controller.busy = true
            verify(!link.enabled && !extract.enabled)
        }
        function test_caption_alignment(data) {
            ribbon.width = data.width
            const controls = buttons()
            for (const button of controls) button.font.pixelSize = data.font
            verify(waitForPolish(ribbon.Window.window))
            const reference = controls[0].mapToItem(ribbon, 0, controls[0].captionBaseline).y
            for (const button of controls) {
                fuzzyCompare(button.mapToItem(ribbon, 0, button.captionBaseline).y, reference, 0.5,
                             "tree action captions share a baseline")
                compare(button.symbolHeight, controls[0].symbolHeight)
                const expectedSize = button.objectName === "manualRoot" ? 10.4 : 16
                const icon = findChild(button, "ribbonSymbolIcon")
                verify(icon)
                compare(icon.width, expectedSize)
                compare(icon.height, expectedSize)
                verify(button.height >= button.implicitHeight)
                verify(button.width >= button.implicitWidth)
            }
            if (data.width === 240) {
                verify(ribbon.overflowing)
                controls[controls.length - 1].forceActiveFocus(Qt.TabFocusReason)
                tryVerify(() => ribbon.scrollPosition > 0)
            }
        }
    }
}
