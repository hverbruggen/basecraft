import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Alignment: edits on the selected block, its columns and rows.
RibbonPage {
    // The main window: the open document and the window's own functions.
    required property var app
    required property var renameDialog
    spacing: 6 * app.ribbonSizeFactor
    RibbonGroup {
        disabledReason: "Open an alignment to use this group"
        enabled: app.alignmentOpen
        title: "Editing mode"
        ModeButton { commandId: "ribbon.insert-typing-inserts-bases-the-rest-of-the-row";
            iconName: "add"
            text: "Insert"
            active: app.alignmentInsert
            tip: "Insert: typing inserts bases; the rest of the row moves right"
            keys: "Insert"
            onInvoked: app.alignmentInsert = true
        }
        ModeButton { commandId: "ribbon.overwrite-typing-replaces-the-selected-bases";
            iconName: "edit"
            text: "Overwrite"
            active: !app.alignmentInsert
            tip: "Overwrite: typing replaces the selected bases"
            keys: "Insert"
            onInvoked: app.alignmentInsert = false
        }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open an alignment to use this group"
        enabled: app.alignmentOpen
        title: "Columns"
        RibbonButton { commandId: "ribbon.delete-columns-containing-only-gaps-across-all-rows-including";
            iconName: "columns"
            text: "Delete empty"
            badge: app.current && app.current.gap_column_count >= 0 ? String(app.current.gap_column_count) : "…"
            enabled: app.current !== null && app.current.gap_column_count > 0
            tip: "Delete columns containing only gaps across all rows, including hidden rows"
            onInvoked: app.current.remove_gap_columns()
        }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open an alignment to use this group"
        enabled: app.alignmentOpen
        title: "Rows"
        RibbonButton { commandId: "ribbon.move-the-selected-rows-up"; iconName: "up"; keys: "Alt+↑"; onInvoked: app.current.move_rows(true) }
        RibbonButton { commandId: "ribbon.move-the-selected-rows-down"; iconName: "down"; keys: "Alt+↓"; onInvoked: app.current.move_rows(false) }
        RibbonButton { commandId: "ribbon.rename-the-cursor-s-row"; Accessible.name: tip; keys: "F2"; onInvoked: renameDialog.dialog().askRow() }
        RibbonButton { commandId: "ribbon.delete-the-selected-rows"; iconName: "delete"; onInvoked: app.current.remove_rows() }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open an alignment to use this group"
        enabled: app.alignmentOpen
        title: "Find"
        RibbonButton { commandId: "alignment.blast"; iconName: "search"; onInvoked: app.beginBlast("all") }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open an alignment to use this group"
        enabled: app.alignmentOpen
        title: "Align"
        RibbonButton { commandId: "ribbon.realign-the-sequences-with-mafft-default-settings-into-a";
            iconName: "align"
            text: app.current && app.current.busy ? "Aligning" : "MAFFT"
            enabled: app.current !== null && !app.current.busy
            tip: "Realign the sequences with MAFFT, default settings, into a new file"
            onInvoked: app.current.align_mafft()
        }
        RibbonButton { commandId: "ribbon.trim-unreliable-columns-with-trimal-automated1-into-a-new";
            iconName: "trim"
            text: "trimAL"
            enabled: app.current !== null && !app.current.busy
            tip: "Trim unreliable columns with trimAl automated1 into a new alignment"
            onInvoked: app.current.trim_alignment()
        }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        disabledReason: "Open an alignment to use this group"
        enabled: app.alignmentOpen
        title: "Add sequence"
        RibbonButton { commandId: "ribbon.type-a-new-sequence-or-paste-fasta"; iconName: "new-file"; enabled: app.current !== null && !app.current.busy; onInvoked: app.documentAt(app.tabs.currentIndex).addText() }
        RibbonButton { commandId: "ribbon.add-all-sequences-from-a-fasta-file"; iconName: "add-file"; enabled: app.current !== null && !app.current.busy; onInvoked: app.documentAt(app.tabs.currentIndex).addFasta() }
        RibbonButton { commandId: "ribbon.review-chromatograms-and-add-their-consensus"; iconName: "waveform"; enabled: app.current !== null && !app.current.busy; onInvoked: app.documentAt(app.tabs.currentIndex).addTraces() }
    }
    Item { Layout.fillWidth: true }
    PageHelpGroup {}
}
