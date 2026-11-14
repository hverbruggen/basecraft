import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Species: a delimitation workspace, its display tree and hypotheses.
RibbonPage {
    // The main window: the open document and the window's own functions.
    required property var app
    required property var specimenMetadata
    required property var rosterConsumer
    required property var treeStyle
    required property var combinePicker
    RibbonGroup {
        id: speciesMethods
        title: "Species delimitation"
        readonly property bool canRun: (app.alignmentOpen || app.treeOpen || app.delimitationOpen) && app.current !== null && !app.current.busy
        RibbonButton { commandId: "species.gmyc-workspace"; iconName: "tree-gmyc"; enabled: speciesMethods.canRun; onInvoked: app.beginDelimitation("gmyc") }
        RibbonButton { commandId: "species.asap-workspace"; iconName: "matrix-asap"; enabled: (app.alignmentOpen || (app.delimitationOpen && app.current && app.current.linked_alignment.length > 0)) && speciesMethods.canRun; onInvoked: app.beginDelimitation("asap") }
        RibbonButton { commandId: "species.ptp-workspace"; iconName: "phylogram-ptp"; enabled: speciesMethods.canRun; onInvoked: app.beginDelimitation("ptp") }
        RibbonButton { commandId: "species.manual-workspace"; iconName: "hypothesis-manual"; enabled: speciesMethods.canRun; onInvoked: app.beginDelimitation("manual") }
        RibbonButton { commandId: "species.combine-workspace"; iconName: "hypothesis-combine"; onInvoked: combinePicker.dialog().open() }
    }
    RibbonGroup {
        title: "Specimens"
        RibbonButton { commandId: "species.roster-links"; iconName: "matrix"; enabled: app.speciesView !== null && specimenMetadata.available; onInvoked: rosterConsumer.dialog().show(app.current, app.currentPath) }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Display"
        ModeButton { commandId: "species.tree-view"; iconName: "phylogram"; enabled: app.speciesView !== null && app.current.display_path.length > 0; active: app.speciesView !== null && !app.speciesView.showGroups; onInvoked: app.speciesView.showGroups = false }
        ModeButton { commandId: "species.text-view"; iconName: "text-representation"; enabled: app.speciesView !== null; active: app.speciesView !== null && app.speciesView.showGroups; onInvoked: app.speciesView.showGroups = true }
        RibbonButton { commandId: "species.layout"; iconName: "bar-layout"; enabled: app.speciesView !== null && app.speciesView.columnInfo.columns.length > 0; onInvoked: app.speciesView.layoutBars() }
    }
    ToolSeparator { Layout.fillHeight: true }
    // The same tree style controls as the tree page, for the
    // display tree; they change only how it is drawn here.
    TreeStyleGroup {
        scope: "species"
        controller: app.speciesTree
        customization: app.customization
        onExtraRequested: treeStyle.dialog().openStyle("extra")
        Component.onCompleted: commands.forEach(command => app.registerRibbonCommand(command))
        RibbonButton { commandId: "species.support-style"; iconName: "support-style"; enabled: app.speciesTree !== null && app.speciesTree.has_support; onInvoked: treeStyle.dialog().openStyle("support") }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Hypotheses"
        RibbonButton { commandId: "species.new"; iconName: "add"; enabled: app.speciesView !== null && !app.current.busy; onInvoked: app.speciesView.newHypothesis() }
        RibbonButton { commandId: "species.columns"; iconName: "columns"; enabled: app.speciesView !== null; onInvoked: app.speciesView.chooseColumns() }
        ModeButton { commandId: "species.details"; iconName: "info"; enabled: app.speciesView !== null; active: app.speciesView !== null && app.speciesView.detailsOpen; onInvoked: app.speciesView.detailsOpen = !app.speciesView.detailsOpen }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Tools"
        ModeButton { commandId: "species.select-tool"; iconName: "selection"; enabled: app.speciesView !== null; active: app.speciesView !== null && app.speciesView.tool === "select"; onInvoked: app.speciesView.setTool("select") }
        ModeButton { commandId: "species.threshold-cut"; tip: app.speciesView !== null && !app.current.can_cut ? "Needs an ultrametric display tree (all tips the same distance from the root)" : "Drag a threshold across the tree and save the species it defines"; iconName: "tree-threshold"; enabled: app.speciesView !== null && !app.current.busy && app.current.can_cut; active: app.speciesView !== null && app.speciesView.tool === "cut"; onInvoked: app.speciesView.setTool("cut") }
        RibbonButton { commandId: "species.display"; tip: app.speciesView !== null && app.current.display_path.length ? "A display tree is attached" : "Attach or build a display tree"; iconName: "tree"; enabled: app.speciesView !== null && !app.current.busy && !app.current.display_path.length; onInvoked: app.beginDelimitation("view") }
    }
    Item { Layout.fillWidth: true }
    PageHelpGroup {}
}
