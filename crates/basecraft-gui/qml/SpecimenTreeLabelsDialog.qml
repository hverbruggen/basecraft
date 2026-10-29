import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppDialog {
    id: metadataTipLabels
    property var metadataController: null
    property var table: ({rows: [], partitions: []})
    property string projectDir: ""
    implicitWidth: formMetrics.averageCharacterWidth * 70 + leftPadding + rightPadding
    FontMetrics { id: formMetrics; font: metadataTipLabels.font }
    property string revision: ""
    property string path: ""
    property var records: []
    property var pendingLinks: ({})
    property var snapshot: null
    function show(controller, file) {
        target = controller; path = file; revision = controller.record_revision()
        const presentation = JSON.parse(controller.style_json())
        snapshot = presentation.style.metadata_labels || null
        const tips = presentation.tips || []
        const result = JSON.parse(metadataTipLabels.metadataController.consumer(JSON.stringify({action: "labels", document: file, records: tips, fields: []}), false))
        records = result.labels || tips.map(record => ({record: record, specimen: null, issue: "No specimen metadata"}))
        pendingLinks = {}; error = result.error || ""; open()
    }
    function applyFields(fields) {
        if (!target || target.record_revision() !== revision) { error = "The tree changed. Close this dialog and choose labels again."; return }
        if (Object.keys(pendingLinks).length) {
            const saved = JSON.parse(metadataController.consumer(JSON.stringify({action: "link_records", document: path, links: pendingLinks}), true))
            if (saved.error) { error = saved.error; return }
            pendingLinks = {}
        }
        const e = target.compose_metadata_labels(metadataTipLabels.projectDir, JSON.stringify(fields))
        if (e.length) error = e; else close()
    }
    title: "Tree tip labels"
    property var target: null
    property string error: ""
    standardButtons: Dialog.Apply | Dialog.Reset | Dialog.Close
    onApplied: applyFields(chosenFields())
    onReset: applyFields([])
    onOpened: standardButton(Dialog.Reset).text = "Original labels"
    function chosenFields() {
        const fields = []
        if (labelGenus.checked) fields.push("genus")
        if (labelSpecies.checked) fields.push("species")
        if (labelSpecimen.checked) fields.push("specimen")
        if (labelAccession.checked) fields.push("accession")
        return fields
    }
    initialFocusItem: labelSpecimen
    contentItem: ColumnLayout {
        implicitWidth: formMetrics.averageCharacterWidth * 70
        implicitHeight: 480
        Label { text: "Compose stored metadata in this order. Empty fields are skipped; unlinked tips keep their identity."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label {
            text: metadataTipLabels.snapshot
                ? "Saved label snapshot from " + ((metadataTipLabels.snapshot.metadata_source || {}).path || "unknown metadata owner") + " (" + metadataTipLabels.snapshot.metadata_hash + "). Metadata edits do not update this snapshot automatically. Apply explicitly refreshes it; Original labels clears the snapshot before rebinding to another project."
                : "Apply saves a label snapshot with its metadata owner and hash. Metadata edits do not update labels automatically."
            wrapMode: Text.Wrap; Layout.fillWidth: true
        }
        Label { text: "Record links"; font.bold: true }
        SpecimenLinkList {
            id: treeLinks
            nextFocusItem: saveLinks.enabled ? saveLinks : labelGenus
            objectName: "treeRecordLinks"
            rows: metadataTipLabels.records.map(r => ({id: r.record, label: r.record + " → " + (r.specimen || r.issue || "Unlinked")}))
            specimenIds: metadataTipLabels.table.rows.map(s => s.id)
            links: metadataTipLabels.pendingLinks
            emptyText: "Leave unchanged"
            enabled: metadataTipLabels.metadataController && metadataTipLabels.metadataController.available
            onEdited: (record, specimen) => {
                const links = Object.assign({}, metadataTipLabels.pendingLinks)
                if (specimen.length) links[record] = specimen; else delete links[record]
                metadataTipLabels.pendingLinks = links
            }
        }
        AppButton { id: saveLinks; Keys.onPressed: event => treeLinks.focusLastOnBacktab(event); objectName: "saveTreeRecordLinks"; text: "Save record links"; enabled: Object.keys(metadataTipLabels.pendingLinks).length > 0; onClicked: {
            if (metadataTipLabels.target.record_revision() !== metadataTipLabels.revision) { metadataTipLabels.error = "The tree changed. Close and reopen this dialog."; return }
            const p = JSON.parse(metadataTipLabels.metadataController.consumer(JSON.stringify({action: "link_records", document: metadataTipLabels.path, links: metadataTipLabels.pendingLinks}), true))
            if (p.error) metadataTipLabels.error = p.error
            else metadataTipLabels.show(metadataTipLabels.target, metadataTipLabels.path)
        } }
        GridLayout { columns: 2
        CheckBox { id: labelGenus; objectName: "metadataLabelGenus"; Keys.onPressed: event => { if (!saveLinks.enabled) treeLinks.focusLastOnBacktab(event); else event.accepted = false }; text: "Genus"; checked: true }
        CheckBox { id: labelSpecies; text: "Species epithet"; checked: true }
        CheckBox { id: labelSpecimen; text: "Specimen ID"; checked: true }
        CheckBox { id: labelAccession; text: "Accession" }
        }
        AppErrorLabel { text: metadataTipLabels.error; visible: text.length > 0; Layout.fillWidth: true }

    }
}
