import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppContentDialog {
    id: dialog
    title: "Protect analysis evidence"
    property var check: ({evidence: {conflict: {analyses: [], options: []}, issues: []}})
    property var conflict: check.evidence.conflict
    readonly property var issues: check.evidence.issues || []
    readonly property bool acknowledgeIncomplete: acknowledgement.checked
    onCheckChanged: acknowledgement.checked = false
    signal retry()
    signal saveAs()
    property string errorText: ""
    signal choice(string policy)
    signal relinkOriginal(string path)
    standardButtons: Dialog.Cancel
    initialFocusItem: standardButton(Dialog.Cancel)
    Shortcut {
        sequences: ["Return", "Enter"]
        enabled: dialog.opened && dialog.standardButton(Dialog.Cancel).activeFocus
        onActivated: dialog.reject()
    }
    body: ColumnLayout {
        spacing: 10
        Label {
            visible: dialog.issues.length > 0
            text: "Basecraft could not check all analyses that may use this file. Saving may make their original input unavailable. This save has not replaced the file."
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        Repeater {
            model: dialog.issues
            Label {
                required property var modelData
                text: modelData.location + ": " + modelData.reason
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
        }
        RowLayout {
            visible: dialog.issues.length > 0
            AppButton { text: "Retry check"; onClicked: dialog.retry() }
            AppButton { text: "Save As…"; onClicked: dialog.saveAs() }
        }
        CheckBox {
            id: acknowledgement
            objectName: "acknowledgeIncompleteEvidence"
            visible: dialog.issues.length > 0
            text: "Save anyway without checking all evidence"
            checked: false
        }
        AppButton {
            objectName: "saveUnchecked"
            visible: dialog.issues.length > 0 && !(dialog.conflict.analyses || []).length
            text: "Save anyway without checking all evidence"
            enabled: acknowledgement.checked
            onClicked: dialog.choice("cancel")
        }
        Label {
            visible: (dialog.conflict.analyses || []).length > 0
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
            visible: (dialog.conflict.analyses || []).length > 0
            text: "Preserve original copy and save"
            enabled: (dialog.conflict.options || []).includes("preserve_copy") && (!dialog.issues.length || acknowledgement.checked)
            Layout.fillWidth: true
            onClicked: dialog.choice("preserve_copy")
        }
        Label {
            visible: (dialog.conflict.analyses || []).length > 0
            text: "Copies the verified old input into hidden analysis metadata before saving. Unavailable originals cannot be repaired by copying changed bytes."
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        AppButton {
            objectName: "removeAnalysis"
            visible: (dialog.conflict.analyses || []).length > 0
            enabled: !dialog.issues.length || acknowledgement.checked
            text: "Remove analysis records and save"
            Layout.fillWidth: true
            onClicked: dialog.choice("remove_analysis")
        }
        Label {
            visible: (dialog.conflict.analyses || []).length > 0
            text: "Moves only affected hidden analysis records to the store's trash. Result files and source files remain."
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        AppButton {
            objectName: "breakEvidence"
            visible: (dialog.conflict.analyses || []).length > 0
            enabled: !dialog.issues.length || acknowledgement.checked
            text: "Break evidence chain and save"
            Layout.fillWidth: true
            onClicked: dialog.choice("break_evidence")
        }
        Label {
            visible: (dialog.conflict.analyses || []).length > 0
            text: "Keeps results and records, marking the original input evidence explicitly unavailable."
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        Label {
            visible: (dialog.conflict.analyses || []).length > 0
            text: "If the input changed elsewhere, relink a file containing the verified original bytes:"
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        TextField {
            id: originalPath
            objectName: "originalInputPath"
            visible: (dialog.conflict.analyses || []).length > 0
            Layout.fillWidth: true
            placeholderText: "Path to the original input"
            Accessible.name: "Path to verified original input"
        }
        AppButton {
            visible: (dialog.conflict.analyses || []).length > 0
            text: "Relink verified original"
            enabled: originalPath.text.length > 0
            onClicked: dialog.relinkOriginal(originalPath.text)
        }
        AppErrorLabel { text: dialog.errorText; visible: text.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap }
    }
}
