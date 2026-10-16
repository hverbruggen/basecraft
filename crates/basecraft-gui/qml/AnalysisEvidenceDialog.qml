import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppContentDialog {
    id: dialog
    title: "Protect analysis evidence"
    property var conflict: ({analyses: [], options: []})
    property string errorText: ""
    signal choice(string policy)
    standardButtons: Dialog.Cancel
    initialFocusItem: standardButton(Dialog.Cancel)
    body: ColumnLayout {
        spacing: 10
        Label {
            text: "Saving this file would replace input used by these analyses:"
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        Repeater {
            model: dialog.conflict.analyses || []
            Label {
                required property var modelData
                text: modelData.result || "Unpublished analysis"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                font.bold: true
            }
        }
        AppButton {
            objectName: "preserveInput"
            text: "Preserve original copy and save"
            enabled: (dialog.conflict.options || []).includes("preserve_copy")
            Layout.fillWidth: true
            onClicked: dialog.choice("preserve_copy")
        }
        Label {
            text: "Copies the verified old input into hidden analysis metadata before saving. Unavailable originals cannot be repaired by copying changed bytes."
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        AppButton {
            objectName: "removeAnalysis"
            text: "Remove analysis records and save"
            Layout.fillWidth: true
            onClicked: dialog.choice("remove_analysis")
        }
        Label {
            text: "Moves only affected hidden analysis records to the store's trash. Result files and source files remain."
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        AppButton {
            objectName: "breakEvidence"
            text: "Break evidence chain and save"
            Layout.fillWidth: true
            onClicked: dialog.choice("break_evidence")
        }
        Label {
            text: "Keeps results and records, marking the original input evidence explicitly unavailable."
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        AppErrorLabel { text: dialog.errorText; visible: text.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap }
    }
}
