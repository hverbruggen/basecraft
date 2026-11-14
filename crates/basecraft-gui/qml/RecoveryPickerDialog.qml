import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Choose a recovery session and where its recovered copies go.
AppFormDialog {
    id: recoveryPickerItem
    required property var app
    required property var destinationFolder
    title: "Recover unsaved work"
    standardButtons: Dialog.Close
    property var sessions: []
    initialFocusItem: recoveryChoice
    Label { text: "Recovery creates copies in a new directory using the unchanged original or a retained recovery file. Originals and journals are preserved."; wrapMode: Text.Wrap; Layout.fillWidth: true }
    Label { visible: !recoveryPickerItem.sessions.length; text: app.provenance.message.startsWith("fail:") ? app.provenance.message : "No recovery sessions are available."; wrapMode: Text.Wrap; Layout.fillWidth: true }
    AppComboBox {
        id: recoveryChoice
        Kirigami.FormData.label: "Session:"
        Layout.fillWidth: true
        model: recoveryPickerItem.sessions.map(session => session.name + " — " + session.id)
        enabled: model.length > 0
    }
    AppButton {
        text: "Choose recovery destination…"
        enabled: recoveryChoice.currentIndex >= 0 && recoveryPickerItem.sessions.length > 0
        onClicked: {
            app.provenanceSource = recoveryPickerItem.sessions[recoveryChoice.currentIndex].path
            app.provenanceExport = "recover"
            recoveryPickerItem.close()
            destinationFolder.dialog().open()
        }
    }
}
