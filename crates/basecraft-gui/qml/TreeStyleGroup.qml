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
    readonly property var commands: [order.commandAction, smallFirst, largeFirst, extra.commandAction, labels.commandAction]
    signal extraRequested()
    signal labelsRequested()
    RibbonNumberField {
        objectName: "tipFont"
        label: "Tips"
        tip: "Tip name size in pixels"
        from: 6; to: 40
        value: group.controller ? group.controller.tip_font : 13
        disabledReason: group.treePage ? "Open a tree to change its style" : "Attach a display tree to the species workspace to change its style"
        enabled: group.controller !== null
        onEdited: value => group.controller.set_sizes(value, 0, 0)
    }
    AppCommandButton {
        id: labels
        commandId: group.scope + ".metadata-labels"
        customization: group.customization
        iconName: "matrix"
        objectName: "metadataTipLabels"
        text: "Labels"
        tip: "Compose tree tip labels from specimen metadata"
        contextVisible: group.treePage
        disabledReason: group.treePage ? "Open a tree to change its style" : "Attach a display tree to the species workspace to change its style"
        enabled: group.controller !== null
        onInvoked: group.labelsRequested()
    }
    AppCommand {
        id: smallFirst
        commandId: group.treePage ? "ribbon.order-every-clade-s-branches-by-size-smallest-first" : group.scope + ".small-on-top"
        customization: group.customization
        text: "Small on top"
        iconName: "tree-small-on-top"
        tooltip: "Order every clade's branches by size, smallest first"
        disabledReason: group.treePage ? "Open a tree to change its style" : "Attach a display tree to the species workspace to change its style"
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
        disabledReason: group.treePage ? "Open a tree to change its style" : "Attach a display tree to the species workspace to change its style"
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
        disabledReason: group.treePage ? "Open a tree to change its style" : "Attach a display tree to the species workspace to change its style"
        enabled: group.controller !== null
        onInvoked: orderMenu.popup(order, 0, order.height)
        RibbonMenu {
            id: orderMenu
            sizeFactor: order.sizeFactor
            RibbonMenuItem { sizeFactor: order.sizeFactor; text: smallFirst.text; onTriggered: smallFirst.trigger() }
            RibbonMenuItem { sizeFactor: order.sizeFactor; text: largeFirst.text; onTriggered: largeFirst.trigger() }
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
        disabledReason: group.treePage ? "Open a tree to change its style" : "Attach a display tree to the species workspace to change its style"
        enabled: group.controller !== null
        onInvoked: group.extraRequested()
    }
}
