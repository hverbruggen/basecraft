import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Tree: the selected clade, the order of clades, support values and figures.
RibbonPage {
    required property var app
    required property var operationWindow
    required property var cladeEditor
    required property var treeStyle
    required property var metadataTipLabels
    required property var supportFile
    required property var linkAlignmentFile
    spacing: 6 * app.ribbonSizeFactor
    TreeRootingGroup {
        controller: app.treeOpen ? app.current : null
        captions: app.commandList
        treeOpen: app.treeOpen
        customization: app.customization
        Component.onCompleted: commands.forEach(command => app.registerRibbonCommand(command))
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open a tree to use this group"
        enabled: app.treeOpen
        title: "Clock"
        RibbonButton {
            commandId: "tree.rate-smoothing"
            iconName: "tree-clock"
            enabled: app.treeOpen && app.current !== null && app.currentPath.length > 0
            onInvoked: operationWindow.dialog().showOperation(app.shell.project_dir.length ? app.shell.project_dir : app.shell.local_file_path(app.shell.parent_folder_url(app.currentPath)), app.currentPath, !!app.current.dirty, "basecraft.sanderson/date_tree@1")
        }
    }
    ToolSeparator { Layout.fillHeight: true }
    TreeCladeGroup {
        controller: app.treeOpen ? app.current : null
        captions: app.commandList
        customization: app.customization
        onAnnotateRequested: cladeEditor.openSelected()
        Component.onCompleted: commands.forEach(command => app.registerRibbonCommand(command))
    }
    ToolSeparator { Layout.fillHeight: true }
    TreeStyleGroup {
        controller: app.treeOpen ? app.current : null
        customization: app.customization
        onExtraRequested: treeStyle.dialog().openStyle("extra")
        onLabelsRequested: metadataTipLabels.dialog().show(app.current, app.currentPath)
        Component.onCompleted: commands.forEach(command => app.registerRibbonCommand(command))
    }
    ToolSeparator { Layout.fillHeight: true }
    // Support values: everything but Add is greyed while the tree has none.
    RibbonGroup {
        disabledReason: "Open a tree to use this group"
        enabled: app.treeOpen
        id: supportGroup
        title: "Support"
        readonly property bool hasSupport: app.treeOpen && app.current !== null && app.current.has_support === true
        RibbonButton { commandId: "ribbon.add-support-values-from-another-analysis-of-the-same"; onInvoked: supportFile.dialog().open() }
        RibbonNumberField {
            objectName: "supportFont"
            label: "Size"
            tip: "Support value size in pixels"
            disabledReason: app.treeOpen ? "Use Support > Add to add tree support values before resizing them" : "Open a tree to resize support values"
            from: 6; to: 30
            value: app.treeOpen && app.current ? app.current.support_font : 10
            enabled: supportGroup.hasSupport
            onEdited: value => app.current.set_sizes(0, value, 0)
        }
        RibbonButton { commandId: "ribbon.which-support-values-show-circles-at-nodes-sizes"; iconName: "support-style"; enabled: supportGroup.hasSupport; onInvoked: treeStyle.dialog().openStyle("support") }
    }
    ToolSeparator { Layout.fillHeight: true }
    TreeAlignmentGroup {
        controller: app.treeOpen ? app.current : null
        captions: app.commandList
        customization: app.customization
        onLinkRequested: (target) => linkAlignmentFile.dialog().ask(target)
        Component.onCompleted: commands.forEach(command => app.registerRibbonCommand(command))
    }
    Item { Layout.fillWidth: true }
    PageHelpGroup {}
}
