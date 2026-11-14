import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Trace: reviewing Sanger calls, issues, masking and the consensus.
RibbonPage {
    // The main window: the open document and the window's own functions.
    required property var app
    // The FASTA motif fits the shared compact symbol band.
    component ExportConsensusButton: RibbonButton {
        id: consensusExportButton
        symbolSize: 16
        Accessible.name: tip
        background: Rectangle {
            radius: 3
            readonly property color ink: consensusExportButton.appWindow ? consensusExportButton.appWindow.palette.windowText : "black"
            color: Qt.alpha(ink, consensusExportButton.pressed ? 0.14 : consensusExportButton.hovered ? 0.10 : 0.05)
            border.color: Qt.alpha(ink, 0.18)
        }
    }
    spacing: 6 * app.ribbonSizeFactor
    RibbonGroup {
        title: "Calls"
        disabledReason: !app.traceOpen || !app.current || !app.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
        enabled: app.traceOpen && app.current && app.current.review_mode && app.traceDocument && !app.traceDocument.working
        AppComboBox {
            id: traceCallSets
            property string keyHint: "T"
            Accessible.name: "Trace call set"
            model: app.traceOpen && app.current ? JSON.parse(app.current.call_sets) : []
            textRole: "label"
            valueRole: "id"
            currentIndex: app.traceOpen && app.current ? model.findIndex(row => row.id === app.current.selected_call_set) : -1
            onActivated: app.current.select_calls(currentValue)
            HoverHandler { id: callSetTipHover }
            Accessible.description: ToolTip.text
            ToolTip.text: !enabled ? (!app.traceOpen || !app.current || !app.current.review_mode ? "Open a Sanger review to choose a call set" : "Wait for trace processing to finish") : "Each call set retains its own reviewed decisions. Computed calls are an alternative, not a guarantee of greater accuracy."
            ToolTip.visible: callSetTipHover.hovered || activeFocus
        }
        RibbonButton { commandId: "trace.compute-calls"; enabled: app.current && !app.current.calling && app.traceDocument && app.traceDocument.traceRows.length === 1; onInvoked: app.current.compute_calls() }
        RibbonButton { commandId: "trace.cancel-calls"; enabled: !!app.current && !!app.current.calling; onInvoked: app.current.cancel_calls() }
    }
    RibbonGroup {
        title: "Issues"
        disabledReason: !app.traceOpen || !app.current || !app.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
        enabled: app.traceOpen && app.traceDocument && !app.traceDocument.working
        RibbonButton { commandId: "ribbon.previous-unresolved-issue"; iconName: "previous"; keys: "Alt+← or ["; enabled: (app.traceOpen && app.current && app.current.review_mode); onInvoked: app.traceDocument.issue(false) }
        RibbonButton { commandId: "ribbon.next-unresolved-issue"; iconName: "next"; keys: "Alt+→ or ]"; enabled: (app.traceOpen && app.current && app.current.review_mode); onInvoked: app.traceDocument.issue(true) }
        RibbonButton { commandId: "ribbon.accept-this-base-and-go-to-the-next-issue"; iconName: "accept"; keys: "Alt+Enter"; enabled: (app.traceOpen && app.current && app.current.review_mode); onInvoked: app.traceDocument.accept() }
        RibbonButton { commandId: "ribbon.choose-alignment-context"; enabled: (app.traceOpen && app.current && app.current.review_mode && app.traceDocument && app.traceDocument.returnAlignment === null); onInvoked: app.traceDocument.alignmentContext() }
        RibbonButton { commandId: "ribbon.combine-up-to-three-reads"; enabled: (app.traceOpen && app.current && app.traceDocument && app.traceDocument.traceRows.length === 1); onInvoked: app.traceDocument.draftReads() }
        RibbonButton { commandId: "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft"; onInvoked: app.traceDocument.signalSettings() }
    }
    RibbonGroup {
        title: "Masking"
        disabledReason: !app.traceOpen || !app.current || !app.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
        enabled: app.traceOpen && app.current && app.current.review_mode && app.traceDocument && !app.traceDocument.working
        RibbonButton { commandId: "ribbon.choose-the-consensus-range-to-keep-in-exports"; onInvoked: app.traceDocument.maskEnds() }
        RibbonButton { commandId: "ribbon.find-partial-terminal-primer-candidates-and-mask-them"; onInvoked: app.traceDocument.setPrimers() }
        RibbonButton { commandId: "trace.keep-from"; iconName: "trace-mask"; onInvoked: app.traceDocument.setMaskStart(app.current.selected_column) }
        RibbonButton { commandId: "trace.keep-through"; iconName: "trace-mask"; onInvoked: app.traceDocument.setMaskEnd(app.current.selected_column) }
    }
    RibbonGroup {
        title: "Export consensus"
        disabledReason: !app.traceOpen || !app.current || !app.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
        enabled: app.traceOpen && app.current && app.current.review_mode && app.traceDocument && !app.traceDocument.working
        ExportConsensusButton { commandId: "ribbon.add-the-edited-consensus-to-its-alignment"; iconName: "consensus-to-alignment"; keys: "Ctrl+Shift+A"; enabled: app.traceDocument !== null && app.traceDocument.returnAlignment !== null && app.current && app.current.unresolved === 0; onInvoked: app.traceDocument.addToAlignment() }
        ExportConsensusButton { commandId: "ribbon.export-the-edited-consensus-as-fasta"; iconName: "fasta-document"; keys: "Ctrl+Shift+E"; enabled: app.current && app.current.unresolved === 0; onInvoked: app.traceDocument.exportConsensus() }
        ExportConsensusButton { commandId: "ribbon.copy-the-edited-consensus-as-fasta"; iconName: "clipboard"; keys: "Ctrl+Shift+C"; enabled: app.current && app.current.unresolved === 0; onInvoked: app.traceDocument.copyConsensus() }
    }
    Item { Layout.fillWidth: true }
    PageHelpGroup {}
}
