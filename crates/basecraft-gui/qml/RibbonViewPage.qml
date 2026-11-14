import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// View: zoom, what the document shows, find and text.
RibbonPage {
    required property var app
    required property var findCommand
    required property var textRepresentationCommand
    spacing: 6 * app.ribbonSizeFactor
    RibbonGroup {
        title: "Zoom"
        disabledReason: app.delimitationOpen ? "Attach a display tree to the species workspace to zoom it" : "Open a document to zoom its view"
        enabled: (app.delimitationOpen ? app.speciesTree : app.current) !== null
        RibbonButton { commandId: "ribbon.zoom-out"; iconName: "zoom-out"; keys: "Ctrl+−"; onInvoked: (app.delimitationOpen ? app.speciesTree : app.current).zoom_out() }
        RibbonButton { commandId: "ribbon.zoom-in"; iconName: "zoom-in"; keys: "Ctrl++"; onInvoked: (app.delimitationOpen ? app.speciesTree : app.current).zoom_in() }
        RibbonButton { commandId: "ribbon.fit";
            text: "Fit"
            iconName: "fit"
            tip: app.alignmentOpen ? "Show the whole width of the alignment" : app.treeOpen ? "Show the whole tree" : "Show the whole sequence"
            keys: "Ctrl+0"
            onInvoked: (app.delimitationOpen ? app.speciesTree : app.current).fit()
        }
        RibbonButton { commandId: "ribbon.zoom-to-the-selection"; text: "Selection"; iconName: "selection"; enabled: app.current !== null && (app.currentKind === "sequence" || app.alignmentOpen); tip: "Zoom to the selection"; onInvoked: app.current.zoom_to_selection() }
        RibbonNumberField {
            objectName: "zoomPercentage"
            label: "%"
            tip: "Zoom percentage"
            disabledReason: app.delimitationOpen ? "Attach a display tree to zoom it" : "Open a document to change its zoom"
            readonly property var controller: app.delimitationOpen ? app.speciesTree : app.current
            from: 0; to: 20000
            value: controller ? parseFloat(controller.zoom_text) : 100
            fractional: true
            enabled: controller !== null
            onEdited: value => { if (controller) controller.set_zoom_percent(value) }
        }
    }
    ToolSeparator { Layout.fillHeight: true; enabled: app.currentKind === "sequence" }
    // Linear and circular layouts exist in the code but are
    // not offered for now; the wrapped layout is the view.
    RibbonGroup {
        title: "Show"
        AppCheckBox {
            property string keyHint: "V"
            text: "Overview"
            checked: app.overviewEnabled
            enabled: app.current !== null
            HoverHandler { id: overviewTipHover }
            Accessible.description: ToolTip.text
            ToolTip.text: enabled ? "Show a compact navigator when the document extends beyond the view" : "Open a document to show its overview"
            ToolTip.visible: overviewTipHover.hovered || activeFocus
            onToggled: app.overviewEnabled = checked
        }
        AppCheckBox {
            property string keyHint: "GC"
            text: "GC content"
            enabled: app.currentKind === "sequence"
            HoverHandler { id: gcTipHover }
            Accessible.description: ToolTip.text
            ToolTip.text: enabled ? "Show the GC content plot" : "Open a sequence to show its GC content plot"
            ToolTip.visible: gcTipHover.hovered || activeFocus
            ToolTip.delay: 400
            checked: app.showGc
            focusPolicy: Qt.StrongFocus
            onToggled: app.showGc = checked
        }
        AppCheckBox {
            objectName: "colorBlindFriendly"
            property string keyHint: "CB"
            text: "Colour-blind colours"
            HoverHandler { id: paletteTipHover }
            Accessible.description: ToolTip.text
            ToolTip.text: "Data colours that stay distinct for colour-blind readers, in the light or dark variant of the appearance. Labels still identify the data."
            ToolTip.visible: paletteTipHover.hovered || activeFocus
            ToolTip.delay: 400
            checked: !!app.preferenceValues.ui && app.preferenceValues.ui.data_color_scheme === "color-blind-friendly"
            focusPolicy: Qt.StrongFocus
            onToggled: app.persistPreference("colorBlindFriendly", checked)
        }
    }
    Item { Layout.fillWidth: true }
    RibbonGroup { title: "Find and text"; actions: [findCommand, textRepresentationCommand] }
    PageHelpGroup {}
}
