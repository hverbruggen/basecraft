import QtQuick
import QtQuick.Controls.Basic

// Tip names, the order of every clade's branches, and the rest of the
// tree's style in a dialog.
RibbonGroup {
    id: group
    title: "Tree style"
    property var controller: null
    // Command IDs are "<scope>.…", so the tree and species pages each keep
    // their own shortcuts. The tree page keeps its original IDs.
    property string scope: "tree"
    readonly property bool treePage: scope === "tree"
    property var customization: ({shortcuts: {}, ribbon_hidden: []})
    readonly property var commands: [order.commandAction, smallFirst, largeFirst, extra.commandAction]
    signal extraRequested()
    RibbonNumberField {
        objectName: "tipFont"
        label: "Tips"
        tip: "Tip name size in pixels"
        from: 6; to: 40
        value: group.controller ? group.controller.tip_font : 13
        enabled: group.controller !== null
        onEdited: value => group.controller.set_sizes(value, 0, 0)
    }
    AppCommand {
        id: smallFirst
        commandId: group.treePage ? "ribbon.order-every-clade-s-branches-by-size-smallest-first" : group.scope + ".small-on-top"
        customization: group.customization
        text: "Small on top"
        iconName: "tree-small-on-top"
        tooltip: "Order every clade's branches by size, smallest first"
        enabled: group.controller !== null
        onTriggered: group.controller.ladderize(true)
    }
    AppCommand {
        id: largeFirst
        commandId: group.treePage ? "ribbon.order-every-clade-s-branches-by-size-largest-first" : group.scope + ".large-on-top"
        customization: group.customization
        text: "Large on top"
        iconName: "tree-large-on-top"
        tooltip: "Order every clade's branches by size, largest first"
        enabled: group.controller !== null
        onTriggered: group.controller.ladderize(false)
    }
    AppCommandButton {
        id: order
        objectName: "orderBranches"
        commandId: group.scope + ".order-branches"
        customization: group.customization
        text: "Order"
        iconName: "tree-small-on-top"
        tip: "Order every clade's branches by size"
        enabled: group.controller !== null
        onInvoked: orderMenu.popup(order, 0, order.height)
        Menu {
            id: orderMenu
            MenuItem { text: smallFirst.text; onTriggered: smallFirst.trigger() }
            MenuItem { text: largeFirst.text; onTriggered: largeFirst.trigger() }
        }
    }
    AppCommandButton {
        id: extra
        objectName: "treeStyleExtra"
        commandId: group.scope + ".style-extra"
        customization: group.customization
        text: "Extra"
        iconName: "tree-style"
        tip: "Branch width, clade label direction and the scale bar"
        enabled: group.controller !== null
        onInvoked: group.extraRequested()
    }
}
