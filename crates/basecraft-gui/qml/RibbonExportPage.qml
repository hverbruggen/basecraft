import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Export: figures, specimen preparation and the complete GenBank workflow.
RibbonPage {
    // The main window: the open document and the window's own functions.
    required property var app
    required property var exportSvgCommand
    required property var exportPngCommand
    required property var zenodoWindow
    required property var zenodoJobs
    required property var zenodoPicker
    required property var specimenTable
    required property var metadataSheetPicker
    required property var metadataTemplateDestination
    required property var submissions
    required property var submissionJobs
    required property var submissionPicker
    spacing: 6 * app.ribbonSizeFactor
    RibbonGroup {
        title: "Figure"
        AppRibbonButton { action: exportSvgCommand; visible: exportSvgCommand.visible; text: app.commandList.ribbon_label(exportSvgCommand.commandId); iconName: "save-as"; tip: exportSvgCommand.effectiveTooltip }
        AppRibbonButton { action: exportPngCommand; visible: exportPngCommand.visible; text: app.commandList.ribbon_label(exportPngCommand.commandId); iconName: "figure-export"; tip: exportPngCommand.effectiveTooltip }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Zenodo"
        RibbonButton { commandId: "export.zenodo"; iconName: "save-as"; tip: "Prepare selected saved files for a Zenodo draft"; enabled: app.shell.project_dir.length > 0; onInvoked: { const zenodo = zenodoWindow.item; if (zenodo && (zenodo.ready || zenodoJobs.busy || zenodo.attempted)) zenodo.open(); else zenodoPicker.dialog().open() } }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Specimens"
        RibbonButton { commandId: "metadata.table"; iconName: "matrix"; tip: "The project's specimens, identifications and collection data"; enabled: app.shell.project_dir.length > 0; onInvoked: specimenTable.dialog().open() }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Import"
        RibbonButton { commandId: "metadata.import-headers"; iconName: "text-representation"; tip: app.metadataFasta.length ? "Read specimen IDs, taxa and values from this file's FASTA headers" : "Open a FASTA file first"; enabled: app.shell.project_dir.length > 0 && app.metadataFasta.length > 0; onInvoked: app.beginMetadataImport("headers", app.metadataFasta) }
        RibbonButton { commandId: "metadata.import-sheet"; iconName: "add-file"; enabled: app.shell.project_dir.length > 0; onInvoked: metadataSheetPicker.dialog().open() }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Records"
        RibbonButton { commandId: "metadata.template"; iconName: "new-file"; tip: app.metadataFasta.length ? "Save an .ods sheet with a row per record of this file, to fill in" : "Open a FASTA file first"; enabled: app.shell.project_dir.length > 0 && app.metadataFasta.length > 0; onInvoked: metadataTemplateDestination.dialog().open() }
        Label {
            objectName: "metadataLinkStatus"
            visible: app.metadataLinks.records !== undefined
            text: app.metadataLinks.records === undefined ? ""
                : app.metadataLinks.unlinked.length === 0 ? "All " + app.metadataLinks.records + " records linked"
                : app.metadataLinks.unlinked.length + " of " + app.metadataLinks.records + " records not linked"
            Accessible.name: text
        }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "GenBank"
        RibbonButton { commandId: "submission.new"; iconName: "new-file"; tip: app.submissionAlignment.length ? "Prepare a GenBank submission from this alignment" : "Open the marker alignment first"; enabled: app.shell.project_dir.length > 0 && app.submissionAlignment.length > 0; onInvoked: app.startSubmission() }
        RibbonButton { commandId: "submission.open"; iconName: "open"; enabled: app.shell.project_dir.length > 0; onInvoked: submissionPicker.dialog().open() }
        RibbonButton { commandId: "submission.prepare"; iconName: "edit"; tip: submissions.available ? "Rows, batch settings, reading frames, issues, package and accessions" : "Start or open a submission first"; enabled: submissions.available; onInvoked: app.showSubmission() }
        RibbonButton { commandId: "submission.validate"; iconName: "accept"; enabled: submissions.available && !submissionJobs.busy; onInvoked: app.validateSubmission() }
        RibbonButton { commandId: "submission.submitter"; iconName: "project"; onInvoked: app.showContacts() }
    }
    Item { Layout.fillWidth: true }
    PageHelpGroup {}
}
