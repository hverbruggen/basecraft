import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Phylogeny: tree inference from the open alignment.
RibbonPage {
    // The main window: the open document and the window's own functions.
    required property var app
    required property var toolSettings
    spacing: 6 * app.ribbonSizeFactor
    RibbonGroup {
        disabledReason: "Open an alignment to use this group"
        enabled: app.alignmentOpen
        title: "Distance tree"
        Repeater {
            model: [{label: "Matrix", value: "matrix"}, {label: "NJ", value: "nj"}, {label: "UPGMA", value: "upgma"}]
            delegate: RibbonButton { commandId: "analysis.distance." + modelData.value;
                required property var modelData
                iconName: modelData.value === "matrix" ? "matrix" : modelData.value === "nj" ? "phylogram" : "tree"
                text: modelData.label
                enabled: app.current !== null && !app.current.busy
                tip: modelData.value === "matrix" ? "Save observed distances as TSV and a reusable cache" : modelData.value === "upgma" ? "Build a rooted, equal-rate UPGMA tree" : "Build a neighbor-joining tree"
                onInvoked: toolSettings.dialog().show(app.current, "distance", modelData.value)
            }
        }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open an alignment to use this group"
        enabled: app.alignmentOpen
        title: "Maximum likelihood"
        RibbonButton { commandId: "ribbon.infer-a-tree-with-fasttree-standard-settings-gtr-for";
            iconName: "tree-fast"
            text: app.current && app.current.busy ? "Running" : "FastTree"
            enabled: app.current !== null && !app.current.busy
            tip: "Infer a tree with FastTree: choose the model, rate variation and support values"
            onInvoked: toolSettings.dialog().show(app.current, "fasttree", "tree")
        }
        RibbonButton { commandId: "ribbon.select-a-model-and-infer-a-maximum-likelihood-tree";
            iconName: "tree-infer"
            text: "IQ-TREE"
            enabled: app.current !== null && !app.current.busy
            tip: "Infer a maximum-likelihood tree: choose the model, rate variation and branch support"
            onInvoked: toolSettings.dialog().show(app.current, "iqtree", "tree")
        }
    }
    Item { Layout.fillWidth: true }
    PageHelpGroup {}
}
