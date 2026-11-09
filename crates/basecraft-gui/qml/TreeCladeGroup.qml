import QtQuick

RibbonGroup {
    id: group
    title: "Clade"
    property var controller: null
    property var customization: ({shortcuts: {}, ribbon_hidden: []})
    readonly property var commands: [annotate.commandAction, flip.commandAction]
    signal annotateRequested()
    AppCommandButton {
        id: annotate
        objectName: "annotateClade"
        commandId: "ribbon.name-the-selected-clade-with-a-bar-or-a"
        customization: group.customization
        text: "Annotate"
        iconName: "clade-annotate"
        tip: "Name the selected clade, with a bar or a coloured field"
        keys: "Enter"
        disabledReason: !group.controller ? "Open a tree to work with its clades" : "Select a clade in the tree to use this command"
        enabled: group.controller !== null && group.controller.has_selection === true
        onInvoked: group.annotateRequested()
    }
    AppCommandButton {
        id: flip
        objectName: "flipSubtree"
        commandId: "ribbon.reverse-the-order-of-the-branches-in-the-selected"
        customization: group.customization
        text: "Flip"
        iconName: "tree-flip"
        tip: "Reverse the order of the branches in the selected subtree (for a tip, its parent's)"
        disabledReason: !group.controller ? "Open a tree to work with its clades" : "Select a clade in the tree to use this command"
        enabled: group.controller !== null && group.controller.has_selection === true
        onInvoked: group.controller.rotate()
    }
}
