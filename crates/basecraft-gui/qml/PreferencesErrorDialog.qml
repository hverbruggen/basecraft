import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Why the app.preferences could not be loaded or saved, with a retry.
AppContentDialog {
    id: preferencesErrorItem
    required property var app
    title: "Preferences could not be loaded or saved"
    standardButtons: Dialog.Close
    body: ColumnLayout {
        AppErrorLabel {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: app.preferences.error + "\n\nThe original app.preferences are retained. If loading failed, changes in this session remain temporary. Correct the app.preferences file or storage problem, then retry."
        }
        AppButton {
            text: "Retry loading app.preferences"
            onClicked: {
                app.loadPreferences()
                if (!app.preferences.error.length) preferencesErrorItem.close()
            }
        }
    }
}
