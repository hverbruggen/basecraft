import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Results of verifying, packaging or recovering saved files.
AppDialog {
    id: provenanceDialogItem
    required property var app
    title: app.provenanceExport === "recover" ? "Recovered work" : "Saved files and exports"
    implicitWidth: 860
    implicitHeight: 600
    standardButtons: Dialog.Close
    initialFocusItem: provenanceText
    contentItem: ColumnLayout {
        Label { visible: app.provenanceExport !== "recover"; text: "File verification describes saved files. Report exports retain the selected document revision."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        AppMessageBar {
            Layout.fillWidth: true
            busy: app.provenance.busy
            workingText: app.provenance.message || "Working on app.provenance"
            message: app.provenance.message
            cancellable: false
        }
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            TextArea { id: provenanceText; text: app.provenance.text; readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap; Accessible.name: "Recorded app.provenance and verification results" }
        }
    }
}
