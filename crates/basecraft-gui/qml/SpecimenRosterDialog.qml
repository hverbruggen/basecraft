import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppDialog {
    id: rosterConsumer
    property var metadataController: null
    property var table: ({rows: [], partitions: []})
    property string projectDir: ""
    implicitWidth: formMetrics.averageCharacterWidth * 70 + leftPadding + rightPadding
    FontMetrics { id: formMetrics; font: rosterConsumer.font }
    property string revision: ""
    title: "Project specimens and partitions"
    standardButtons: Dialog.Close
    property var target: null
    property string path: ""
    property var work: ({specimens: [], hypotheses: []})
    property var links: ({})
    property string error: ""
    initialFocusItem: partitionSource
    function show(controller, file) {
        target = controller; revision = controller.record_revision(); path = file; work = JSON.parse(controller.workspace_json)
        const stored = {}
        for (const s of work.specimens) if ((s.attributes || {}).basecraft_roster_id) stored[s.id] = s.attributes.basecraft_roster_id
        links = stored; error = ""; open()
    }
    contentItem: ColumnLayout {
        implicitWidth: formMetrics.averageCharacterWidth * 70
        implicitHeight: 480
        Label { text: "Link each workspace ID to its stable project specimen. Several records may share one specimen. Save the workspace before promoting a result."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        SpecimenLinkList {
            objectName: "workspaceRosterLinks"
            rows: rosterConsumer.work.specimens.map(s => ({id: s.id, label: s.id + (rosterConsumer.links[s.id] && !rosterConsumer.table.rows.some(r => r.id === rosterConsumer.links[s.id]) ? " (unknown roster ID: " + rosterConsumer.links[s.id] + ")" : "")}))
            specimenIds: rosterConsumer.table.rows.map(s => s.id)
            links: rosterConsumer.links
            onEdited: (record, specimen) => {
                const updated = Object.assign({}, rosterConsumer.links)
                if (specimen.length) updated[record] = specimen; else delete updated[record]
                rosterConsumer.links = updated
            }
        }
        AppButton { objectName: "applyRosterLinks"; text: "Apply roster links"; onClicked: {
            if (rosterConsumer.target.record_revision() !== rosterConsumer.revision) { rosterConsumer.error = "The workspace changed. Close and reopen this dialog before linking."; return }
            const p = JSON.parse(rosterConsumer.metadataController.consumer(JSON.stringify({action: "link_workspace", workspace: rosterConsumer.path, links: rosterConsumer.links}), false))
            if (p.error) rosterConsumer.error = p.error
            else { rosterConsumer.target.action("roster_links", JSON.stringify(p.operation.params.edit)); rosterConsumer.revision = rosterConsumer.target.record_revision(); rosterConsumer.error = "Links applied. Save the workspace before promoting a partition." }
        } }
        ComboBox { id: partitionSource; objectName: "partitionSource"; model: rosterConsumer.work.hypotheses; textRole: "name"; Accessible.name: "Delimitation result to promote"; Layout.fillWidth: true }
        TextField { id: projectPartitionId; objectName: "projectPartitionId"; placeholderText: "Project partition ID"; Accessible.name: "Project partition ID"; Layout.fillWidth: true }
        AppButton { objectName: "saveProjectPartition"; text: "Save project partition"; enabled: rosterConsumer.target !== null && !rosterConsumer.target.dirty && partitionSource.currentIndex >= 0 && projectPartitionId.text.trim().length > 0; onClicked: {
            const p = JSON.parse(rosterConsumer.metadataController.consumer(JSON.stringify({action: "promote_partition", workspace: rosterConsumer.path, hypothesis: rosterConsumer.work.hypotheses[partitionSource.currentIndex].id, id: projectPartitionId.text.trim()}), true))
            rosterConsumer.error = p.error || "Saved project partition"
        } }
        Label { text: "Project partitions: " + ((rosterConsumer.table.partitions || []).map(p => p.name).join(", ")); wrapMode: Text.Wrap; Layout.fillWidth: true }
        AppErrorLabel { text: rosterConsumer.error; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true }
    }
}
