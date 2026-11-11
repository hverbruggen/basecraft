import QtQuick

RibbonGroup {
    id: group
    title: "Root"
    property var controller: null
    property bool treeOpen: false
    property var customization: ({shortcuts: {}, ribbon_hidden: []})
    // The window's command list (AppCommands): captions and tips.
    property var captions: null
    readonly property var commands: [manual.commandAction, midpoint.commandAction, variance.commandAction]
    AppCommandButton {
        id: manual
        objectName: "manualRoot"
        commandId: "ribbon.root-the-tree-on-the-branch-above-the-selection"
        customization: group.customization
        text: group.captions ? group.captions.caption(commandId) : ""
        iconName: "left"
        symbolSize: 10.4
        tip: group.captions ? group.captions.tip(commandId) : ""
        disabledReason: !group.treeOpen || !group.controller ? "Open a tree to change its root" : group.controller.busy ? "Wait for the tree operation to finish" : commandId === "ribbon.root-the-tree-on-the-branch-above-the-selection" ? "Select a clade or branch to reroot the tree" : "Use a tree with complete nonnegative branch lengths and unique tip names for automatic rooting"
        enabled: group.treeOpen && group.controller !== null && group.controller.has_selection === true
        onInvoked: group.controller.reroot()
    }
    AppCommandButton {
        id: midpoint
        objectName: "midpointRoot"
        commandId: "tree.midpoint_root"
        customization: group.customization
        text: group.captions ? group.captions.caption(commandId) : ""
        iconName: "tree-midpoint"
        tip: group.captions ? group.captions.tip(commandId) : ""
        disabledReason: !group.treeOpen || !group.controller ? "Open a tree to change its root" : group.controller.busy ? "Wait for the tree operation to finish" : commandId === "ribbon.root-the-tree-on-the-branch-above-the-selection" ? "Select a clade or branch to reroot the tree" : "Use a tree with complete nonnegative branch lengths and unique tip names for automatic rooting"
        enabled: group.treeOpen && group.controller !== null && group.controller.can_auto_root === true && !group.controller.busy
        onInvoked: group.controller.midpoint_root()
    }
    AppCommandButton {
        id: variance
        objectName: "minimumVarianceRoot"
        commandId: "tree.minimum_variance_root"
        customization: group.customization
        text: group.captions ? group.captions.caption(commandId) : ""
        iconName: "tree-variance"
        tip: group.captions ? group.captions.tip(commandId) : ""
        disabledReason: !group.treeOpen || !group.controller ? "Open a tree to change its root" : group.controller.busy ? "Wait for the tree operation to finish" : commandId === "ribbon.root-the-tree-on-the-branch-above-the-selection" ? "Select a clade or branch to reroot the tree" : "Use a tree with complete nonnegative branch lengths and unique tip names for automatic rooting"
        enabled: group.treeOpen && group.controller !== null && group.controller.can_auto_root === true && !group.controller.busy
        onInvoked: group.controller.minimum_variance_root()
    }
}
