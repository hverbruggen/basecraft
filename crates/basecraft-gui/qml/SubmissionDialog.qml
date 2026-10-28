import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// A GenBank marker submission: batch settings, authors and title, a
// decision and reviewed reading frame per row, every issue (selecting one
// selects its row), the package and its table2asn validation, and returned
// accessions. Each change is one undoable edit, saved at once by the
// controller.
AppContentDialog {
    id: dialog
    objectName: "submissionDialog"
    title: "GenBank submission"
    standardButtons: Dialog.Close
    initialFocusItem: markerKind
    // SubmissionController.document, .review and .package, parsed.
    property var document: null
    property var review: ({ rows: [], issues: [] })
    property var pkg: null
    property var geneticCodes: []
    property string path: ""
    property string message: ""
    property string messageKind: ""
    property bool canUndo: false
    property bool canRedo: false
    // A table2asn job is running.
    property bool validating: false
    // The accession list matched against the records, not yet recorded.
    property var accessionPlan: null
    property int selectedRow: -1
    signal editRequested(string edit)
    signal publicationRequested(string title, string authors)
    signal decisionRequested(string record, string decision, string accession)
    signal frameRequested(string record, int codonStart)
    signal confirmDetectedRequested()
    signal undoRequested()
    signal redoRequested()
    signal exportRequested()
    signal validateRequested()
    signal cancelValidationRequested()
    signal accessionsMatchRequested(string text)
    signal accessionsRecordRequested(string accessions)
    signal accessionTableRequested()

    readonly property var batch: document ? document.batch : null
    readonly property string kind: batch ? batch.marker.kind : "rrna"
    readonly property bool coding: kind === "coding"
    readonly property var markerKinds: [["coding", "Protein-coding gene"], ["rrna", "Ribosomal RNA gene"],
        ["its", "ITS region"], ["spacer", "Non-coding region"]]
    readonly property var organelles: [["", "None (nuclear)"], ["mitochondrion", "Mitochondrion"],
        ["chloroplast", "Chloroplast"], ["plastid", "Plastid"], ["apicoplast", "Apicoplast"],
        ["chromoplast", "Chromoplast"], ["cyanelle", "Cyanelle"], ["kinetoplast", "Kinetoplast"],
        ["nucleomorph", "Nucleomorph"]]
    readonly property var endPolicies: [["auto", "From start and stop codons"], ["partial", "Both partial"],
        ["complete", "Both complete"]]
    readonly property var itsComponents: [["SSU", "Small subunit rRNA"], ["ITS1", "ITS1"],
        ["5.8S", "5.8S rRNA"], ["ITS2", "ITS2"], ["LSU", "Large subunit rRNA"]]
    readonly property var severityNames: ({ blocking: "Must fix", review: "Check", info: "Note", unavailable: "Not checked" })

    FontMetrics { id: cellMetrics; font: dialog.font }
    readonly property real nameWidth: cellMetrics.averageCharacterWidth * 16
    readonly property real cellWidth: cellMetrics.averageCharacterWidth * 20

    function indexOf(pairs, value) { return Math.max(0, pairs.findIndex(p => p[0] === value)) }
    // The batch settings as the controls show them.
    function batchFromControls() {
        const marker = { kind: markerKinds[markerKind.currentIndex][0] }
        if (marker.kind === "coding") { marker.gene = gene.text.trim(); marker.product = product.text.trim() }
        else if (marker.kind === "rrna") marker.product = product.text.trim()
        else if (marker.kind === "its") marker.components = itsComponents.map(c => c[0]).filter(c => itsChecks.checked(c))
        else marker.description = product.text.trim()
        const out = { marker: marker, mol_type: molType.currentIndex === 1 ? "genomic_rna" : "genomic_dna",
            genetic_code: geneticCodes.length ? geneticCodes[geneticCode.currentIndex].id : 1,
            ends: endPolicies[ends.currentIndex][0] }
        const location = organelles[organelle.currentIndex][0]
        if (location.length) out.organelle = location
        return out
    }
    function saveBatch() {
        if (!document) return
        const next = batchFromControls()
        if (JSON.stringify(next) !== JSON.stringify(batch))
            editRequested(JSON.stringify({ action: "set_batch", batch: next }))
    }
    function setAllDecisions(decision) {
        editRequested(JSON.stringify({action: "set_rows",
            rows: review.rows.map(r => ({record: r.record, decision: decision}))}))
    }
    function selectRow(row) {
        selectedRow = row
        const item = rowsRepeater.itemAt(row)
        if (item) item.focusDecision()
    }
    function frameText(frame) {
        if (!frame) return ""
        if (frame.chosen) return "codon_start " + frame.chosen + (frame.chosen === frame.detected ? " (confirmed)" : " (chosen)")
        if (frame.detected) return "Detected " + frame.detected + ": confirm"
        if (frame.candidates.length) return "Choose: " + frame.candidates.join(" or ")
        return "No clean frame"
    }

    body: ColumnLayout {
        spacing: 8
        Label {
            objectName: "submissionStatus"
            text: !dialog.document ? "No submission is open."
                : dialog.path + " · source " + (dialog.document.source ? dialog.document.source.document : "none")
                    + " · " + (dialog.review.exported || 0) + " records to submit · "
                    + (dialog.review.blocking || 0) + " to fix"
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        AppErrorLabel {
            objectName: "submissionMessage"
            visible: text.length > 0
            text: dialog.message
            color: dialog.messageKind === "error" ? "#c0392b" : palette.windowText
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        RowLayout {
            spacing: 6
            AppButton { text: "Undo"; enabled: dialog.canUndo; onClicked: dialog.undoRequested() }
            AppButton { text: "Redo"; enabled: dialog.canRedo; onClicked: dialog.redoRequested() }
        }

        Label { text: "Batch"; font.bold: true }
        AppFormRow {
            AppComboBox {
                id: markerKind
                objectName: "markerKind"
                Accessible.name: "Marker type"
                model: dialog.markerKinds.map(k => k[1])
                currentIndex: dialog.indexOf(dialog.markerKinds, dialog.kind)
                onActivated: dialog.saveBatch()
            }
            TextField {
                id: gene
                objectName: "markerGene"
                visible: markerKind.currentIndex === 0
                placeholderText: "Gene symbol, such as rbcL"
                Accessible.name: "Gene symbol"
                text: dialog.batch && dialog.batch.marker.gene !== undefined ? dialog.batch.marker.gene : ""
                onEditingFinished: dialog.saveBatch()
            }
            TextField {
                id: product
                objectName: "markerProduct"
                visible: markerKind.currentIndex !== 2
                Layout.preferredWidth: dialog.cellWidth * 2
                placeholderText: markerKind.currentIndex === 3 ? "Region, such as trnL-trnF intergenic spacer" : "Product name"
                Accessible.name: markerKind.currentIndex === 3 ? "Region description" : "Product name"
                text: !dialog.batch ? "" : dialog.batch.marker.product !== undefined ? dialog.batch.marker.product
                    : dialog.batch.marker.description !== undefined ? dialog.batch.marker.description : ""
                onEditingFinished: dialog.saveBatch()
            }
        }
        RowLayout {
            id: itsChecks
            visible: markerKind.currentIndex === 2
            spacing: 6
            function checked(component) {
                for (let i = 0; i < itsRepeater.count; i++)
                    if (dialog.itsComponents[i][0] === component) return itsRepeater.itemAt(i).checked
                return false
            }
            Label { text: "Components present:" }
            Repeater {
                id: itsRepeater
                model: dialog.itsComponents
                delegate: AppCheckBox {
                    required property var modelData
                    objectName: "its_" + modelData[0]
                    text: modelData[1]
                    checked: dialog.batch && dialog.batch.marker.components !== undefined
                        && dialog.batch.marker.components.includes(modelData[0])
                    onToggled: dialog.saveBatch()
                }
            }
        }
        AppFormRow {
            AppComboBox {
                id: molType
                Accessible.name: "Molecule type"
                model: ["Genomic DNA", "Genomic RNA"]
                currentIndex: dialog.batch && dialog.batch.mol_type === "genomic_rna" ? 1 : 0
                onActivated: dialog.saveBatch()
            }
            AppComboBox {
                id: organelle
                objectName: "organelle"
                Accessible.name: "Organelle"
                model: dialog.organelles.map(o => o[1])
                currentIndex: dialog.indexOf(dialog.organelles, dialog.batch && dialog.batch.organelle ? dialog.batch.organelle : "")
                onActivated: dialog.saveBatch()
            }
            AppComboBox {
                id: geneticCode
                objectName: "geneticCode"
                Accessible.name: "Genetic code"
                model: dialog.geneticCodes.map(c => c.name)
                currentIndex: Math.max(0, dialog.geneticCodes.findIndex(c => dialog.batch && c.id === dialog.batch.genetic_code))
                onActivated: dialog.saveBatch()
            }
            AppComboBox {
                id: ends
                Accessible.name: "Feature ends"
                model: dialog.endPolicies.map(e => e[1])
                currentIndex: dialog.indexOf(dialog.endPolicies, dialog.batch ? dialog.batch.ends || "auto" : "auto")
                onActivated: dialog.saveBatch()
            }
        }
        AppCheckBox {
            objectName: "trimUnknown"
            text: "Trim unknown bases (N, ?) at the ends of each sequence"
            checked: !dialog.document || dialog.document.export.trim_terminal_unknown
            onToggled: dialog.editRequested(JSON.stringify({ action: "set_export", export: { trim_terminal_unknown: checked } }))
        }

        Label { text: "Authors and title"; font.bold: true }
        TextField {
            id: pubTitle
            objectName: "publicationTitle"
            Layout.fillWidth: true
            placeholderText: "Publication title"
            Accessible.name: "Publication title"
            text: dialog.document ? dialog.document.publication.title : ""
        }
        TextArea {
            id: authors
            objectName: "publicationAuthors"
            Layout.fillWidth: true
            placeholderText: "One author per line: Last, First M."
            Accessible.name: "Authors, one per line as Last, First M."
            text: dialog.document ? dialog.document.authors_text : ""
            KeyNavigation.priority: KeyNavigation.BeforeItem
            KeyNavigation.tab: savePublication
        }
        AppButton {
            id: savePublication
            objectName: "savePublication"
            text: "Save authors and title"
            onClicked: dialog.publicationRequested(pubTitle.text, authors.text)
        }

        RowLayout {
            spacing: 6
            Label { text: "Rows"; font.bold: true }
            AppButton {
                objectName: "confirmFrames"
                visible: dialog.coding
                text: "Confirm detected frames"
                enabled: dialog.review.rows.some(r => r.frame && !r.frame.chosen && r.frame.detected)
                onClicked: dialog.confirmDetectedRequested()
            }
        }
        Label {
            objectName: "selectionStatus"
            text: dialog.review.selection_confirmed ? "Row selection confirmed."
                : "Review the suggested choices, then confirm the row selection before export. Accession candidates are unverified heuristics; no archive lookup is performed."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        RowLayout {
            spacing: 6
            AppButton {
                objectName: "includeAllRows"
                text: "Include all"
                enabled: !!dialog.review.selection_hash && !dialog.validating
                onClicked: dialog.setAllDecisions("new")
            }
            AppButton {
                objectName: "excludeAllRows"
                text: "Exclude all"
                enabled: !!dialog.review.selection_hash && !dialog.validating
                onClicked: dialog.setAllDecisions("exclude")
            }
            AppButton {
                objectName: "confirmSelection"
                text: "Confirm row selection"
                enabled: !!dialog.review.selection_hash && !dialog.review.selection_confirmed && !dialog.validating
                onClicked: dialog.editRequested(JSON.stringify({action: "confirm_selection",
                    selection_hash: dialog.review.selection_hash}))
            }
        }
        ScrollView {
            Layout.fillWidth: true
            implicitHeight: rowsColumn.implicitHeight + 16
            contentWidth: rowsColumn.implicitWidth
            ScrollBar.vertical.policy: ScrollBar.AlwaysOff
            ColumnLayout {
                id: rowsColumn
                spacing: 2
                RowLayout {
                    spacing: 4
                    Label { text: "Record"; font.bold: true; Layout.preferredWidth: dialog.nameWidth }
                    Label { text: "Specimen and organism"; font.bold: true; Layout.preferredWidth: dialog.cellWidth * 1.5 }
                    Label { text: "Decision"; font.bold: true; Layout.preferredWidth: dialog.cellWidth }
                    Label { text: "Selection reason / evidence"; font.bold: true; Layout.preferredWidth: dialog.cellWidth * 2 }
                    Label { text: "Reading frame"; font.bold: true; visible: dialog.coding; Layout.preferredWidth: dialog.cellWidth * 1.5 }
                    Label { text: "Issues"; font.bold: true }
                }
                Repeater {
                    id: rowsRepeater
                    model: dialog.review.rows
                    delegate: RowLayout {
                        id: row
                        required property var modelData
                        required property int index
                        readonly property bool selected: dialog.selectedRow === index
                        spacing: 4
                        function focusDecision() { decision.forceActiveFocus() }
                        Label {
                            text: row.modelData.record
                            font.bold: row.selected
                            elide: Text.ElideRight
                            Layout.preferredWidth: dialog.nameWidth
                        }
                        Label {
                            text: row.modelData.specimen.length
                                ? row.modelData.specimen + " · " + row.modelData.organism
                                : row.modelData.decision === "new" ? "Not linked" : ""
                            elide: Text.ElideRight
                            Layout.preferredWidth: dialog.cellWidth * 1.5
                            ToolTip.text: row.modelData.research_name.length ? "Research name: " + row.modelData.research_name : ""
                        }
                        AppComboBox {
                            id: decision
                            objectName: "decision_" + row.modelData.record
                            Accessible.name: "Decision for " + row.modelData.record
                            model: ["New sequence", "Existing accession", "Exclude"]
                            currentIndex: ["new", "existing", "exclude"].indexOf(row.modelData.decision)
                            onFocusChanged: if (activeFocus) dialog.selectedRow = row.index
                            onActivated: index => {
                                if (index === 1) { accession.forceActiveFocus(); return }
                                dialog.decisionRequested(row.modelData.record, ["new", "existing", "exclude"][index], "")
                            }
                        }
                        TextField {
                            id: accession
                            objectName: "accession_" + row.modelData.record
                            visible: decision.currentIndex === 1 || row.modelData.decision === "existing"
                            placeholderText: "Accession"
                            Accessible.name: "Existing accession of " + row.modelData.record
                            text: row.modelData.decision === "existing" ? row.modelData.accession : ""
                            Layout.preferredWidth: dialog.cellWidth * 0.7
                            onEditingFinished: if (text.trim().length && text.trim() !== row.modelData.accession)
                                dialog.decisionRequested(row.modelData.record, "existing", text.trim())
                        }
                        Label {
                            objectName: "selectionReason_" + row.modelData.record
                            text: (row.modelData.selection_reason || "")
                                + (row.modelData.accession_candidates || []).map(c => "\n" + c.accession + " · " + c.source + " (unverified)").join("")
                            Accessible.name: "Selection reason for " + row.modelData.record
                            wrapMode: Text.Wrap
                            Layout.preferredWidth: dialog.cellWidth * 2
                        }
                        RowLayout {
                            visible: dialog.coding && row.modelData.frame !== null && row.modelData.frame !== undefined
                            spacing: 4
                            Layout.preferredWidth: dialog.cellWidth * 1.5
                            Label {
                                objectName: "frameText_" + row.modelData.record
                                text: dialog.frameText(row.modelData.frame)
                            }
                            AppComboBox {
                                id: frameChoice
                                objectName: "frame_" + row.modelData.record
                                Accessible.name: "Reading frame (codon_start) of " + row.modelData.record
                                model: ["1", "2", "3"]
                                currentIndex: row.modelData.frame && row.modelData.frame.effective ? row.modelData.frame.effective - 1 : 0
                                onFocusChanged: if (activeFocus) dialog.selectedRow = row.index
                                onActivated: index => dialog.frameRequested(row.modelData.record, index + 1)
                            }
                            AppButton {
                                objectName: "confirmFrame_" + row.modelData.record
                                text: "Confirm"
                                visible: !!row.modelData.frame && !row.modelData.frame.chosen
                                Accessible.name: "Confirm reading frame of " + row.modelData.record
                                onClicked: dialog.frameRequested(row.modelData.record, frameChoice.currentIndex + 1)
                            }
                        }
                        Label {
                            text: row.modelData.blocking ? row.modelData.blocking + " to fix"
                                : row.modelData.review ? row.modelData.review + " to check" : ""
                        }
                    }
                }
            }
        }
        TextArea {
            objectName: "translation"
            readonly property var rowData: dialog.selectedRow >= 0 && dialog.selectedRow < dialog.review.rows.length
                ? dialog.review.rows[dialog.selectedRow] : null
            visible: dialog.coding && rowData !== null && !!rowData.frame && rowData.frame.translation.length > 0
            readOnly: true
            wrapMode: TextEdit.WrapAnywhere
            font.family: "monospace"
            Layout.fillWidth: true
            Accessible.name: "Translation of the selected record"
            text: visible ? rowData.record + " translation (codon_start " + rowData.frame.effective + "): "
                + rowData.frame.translation : ""
        }

        Label { text: "Issues (" + dialog.review.issues.length + ")"; font.bold: true }
        Label {
            visible: dialog.review.issues.length === 0
            text: "No issues found."
        }
        Repeater {
            model: dialog.review.issues
            delegate: ItemDelegate {
                id: issue
                required property var modelData
                required property int index
                objectName: "issue_" + index
                Layout.fillWidth: true
                padding: 2
                contentItem: Label {
                    text: (dialog.severityNames[issue.modelData.severity] || issue.modelData.severity) + " · "
                        + (issue.modelData.record ? issue.modelData.record + ": " : "")
                        + issue.modelData.message
                        + (issue.modelData.source === "basecraft" ? "" : " [" + issue.modelData.source + " " + issue.modelData.code + "]")
                    color: issue.modelData.severity === "blocking" ? "#c0392b" : palette.windowText
                    wrapMode: Text.Wrap
                }
                Accessible.name: contentItem.text
                enabled: issue.modelData.row !== null && issue.modelData.row !== undefined
                onClicked: dialog.selectRow(issue.modelData.row)
            }
        }

        Label { text: "Package and validation"; font.bold: true }
        Label {
            objectName: "packageStatus"
            text: !dialog.pkg || !dialog.pkg.exported ? "Not exported yet."
                : "Package " + dialog.pkg.directory + ": " + dialog.pkg.status
                    + (dialog.pkg.validation ? ". table2asn " + dialog.pkg.validation.tool_version + " " + dialog.pkg.validation.status
                        + " (" + dialog.pkg.validation.errors + " errors, " + dialog.pkg.validation.warnings + " warnings)"
                        : ". Not validated for these files.")
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Label {
            text: "Validation runs table2asn on this computer. A clean report means the files pass its checks; GenBank curators still review every submission."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        RowLayout {
            spacing: 6
            AppButton { objectName: "exportPackage"; text: "Export package"; enabled: !!dialog.document && !!dialog.review.selection_confirmed && !dialog.validating; onClicked: dialog.exportRequested() }
            AppButton { objectName: "validatePackage"; text: "Validate"; enabled: !!dialog.pkg && dialog.pkg.exported && !dialog.validating; onClicked: dialog.validateRequested() }
            AppButton { text: "Stop validation"; visible: dialog.validating; onClicked: dialog.cancelValidationRequested() }
        }

        Label { text: "Accessions"; font.bold: true }
        TextArea {
            id: accessionList
            objectName: "accessionList"
            Layout.fillWidth: true
            placeholderText: "Paste the accession list NCBI sent: one sequence ID and accession per line"
            Accessible.name: "Accession list from NCBI"
            KeyNavigation.priority: KeyNavigation.BeforeItem
            KeyNavigation.tab: matchAccessions
        }
        RowLayout {
            spacing: 6
            AppButton {
                id: matchAccessions
                objectName: "matchAccessions"
                text: "Match to records"
                enabled: accessionList.text.trim().length > 0
                onClicked: dialog.accessionsMatchRequested(accessionList.text)
            }
            AppButton {
                objectName: "recordAccessions"
                text: "Record " + (dialog.accessionPlan ? dialog.accessionPlan.accessions.length : 0) + " accessions"
                enabled: !!dialog.accessionPlan && dialog.accessionPlan.accessions.length > 0
                onClicked: { dialog.accessionsRecordRequested(JSON.stringify(dialog.accessionPlan.accessions)); dialog.accessionPlan = null }
            }
            AppButton { text: "Save accession table"; enabled: !!dialog.document; onClicked: dialog.accessionTableRequested() }
        }
        Label {
            visible: !!dialog.accessionPlan && dialog.accessionPlan.unmatched.length > 0
            text: dialog.accessionPlan ? "Not matched: " + dialog.accessionPlan.unmatched.join("; ") : ""
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
    }
}
