import QtQuick

RibbonGroup {
    id: group
    title: "Clade"
    property var controller: null
    property var customization: ({shortcuts: {}, ribbon_hidden: []})
    // The window's command list (AppCommands): captions and tips.
    property var captions: null
    readonly property var commands: [annotate.commandAction, flip.commandAction]
    signal annotateRequested()
    AppCommandButton {
        id: annotate
        objectName: "annotateClade"
        commandId: "ribbon.name-the-selected-clade-with-a-bar-or-a"
        customization: group.customization
        text: group.captions ? group.captions.caption(commandId) : ""
        iconName: "clade-annotate"
        tip: group.captions ? group.captions.tip(commandId) : ""
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
        text: group.captions ? group.captions.caption(commandId) : ""
        iconName: "tree-flip"
        tip: group.captions ? group.captions.tip(commandId) : ""
        disabledReason: !group.controller ? "Open a tree to work with its clades" : "Select a clade in the tree to use this command"
        enabled: group.controller !== null && group.controller.has_selection === true
        onInvoked: group.controller.rotate()
    }
}
