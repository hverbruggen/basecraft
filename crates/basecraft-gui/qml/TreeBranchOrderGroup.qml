import QtQuick

RibbonGroup {
    id: group
    title: "Branch order"
    property var controller: null
    property var customization: ({shortcuts: {}, ribbon_hidden: []})
    readonly property var commands: [small.commandAction, large.commandAction]
    AppCommandButton {
        id: small
        objectName: "smallBranchesOnTop"
        commandId: "ribbon.order-every-clade-s-branches-by-size-smallest-first"
        customization: group.customization
        text: "Small on top"
        iconName: "tree-small-on-top"
        tip: "Order every clade's branches by size, smallest first"
        enabled: group.controller !== null
        onInvoked: group.controller.ladderize(true)
    }
    AppCommandButton {
        id: large
        objectName: "largeBranchesOnTop"
        commandId: "ribbon.order-every-clade-s-branches-by-size-largest-first"
        customization: group.customization
        text: "Large on top"
        iconName: "tree-large-on-top"
        tip: "Order every clade's branches by size, largest first"
        enabled: group.controller !== null
        onInvoked: group.controller.ladderize(false)
    }
}
