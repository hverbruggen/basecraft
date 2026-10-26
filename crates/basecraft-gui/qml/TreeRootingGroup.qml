import QtQuick

RibbonGroup {
    id: group
    title: "Root"
    property var controller: null
    property bool treeOpen: false
    property var customization: ({shortcuts: {}, ribbon_hidden: []})
    readonly property var commands: [manual.commandAction, midpoint.commandAction, variance.commandAction]
    AppCommandButton {
        id: manual
        objectName: "manualRoot"
        commandId: "ribbon.root-the-tree-on-the-branch-above-the-selection"
        customization: group.customization
        text: "Reroot"
        iconName: "left"
        symbolSize: 10.4
        tip: "Root the tree on the branch above the selection"
        enabled: group.treeOpen && group.controller !== null && group.controller.has_selection === true
        onInvoked: group.controller.reroot()
    }
    AppCommandButton {
        id: midpoint
        objectName: "midpointRoot"
        commandId: "tree.midpoint_root"
        customization: group.customization
        text: "Midpoint"
        iconName: "tree"
        tip: "Root halfway along a longest tip-to-tip branch-length path over the whole tree. Requires complete nonnegative lengths and unique tip names; does not infer dates or make the tree ultrametric."
        enabled: group.treeOpen && group.controller !== null && group.controller.can_auto_root === true && !group.controller.busy
        onInvoked: group.controller.midpoint_root()
    }
    AppCommandButton {
        id: variance
        objectName: "minimumVarianceRoot"
        commandId: "tree.minimum_variance_root"
        customization: group.customization
        text: "MRTTV"
        iconName: "tree"
        tip: "Minimize unweighted root-to-tip path length variance over ALL tips, globally across branch interiors and nodes. Requires complete nonnegative lengths and unique tip names; does not infer dates or make the tree ultrametric."
        enabled: group.treeOpen && group.controller !== null && group.controller.can_auto_root === true && !group.controller.busy
        onInvoked: group.controller.minimum_variance_root()
    }
}
