import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

AppDialog {
    id: dialog
    objectName: "tasksDialog"
    title: "Tasks & notifications"
    modal: false
    implicitWidth: 780
    implicitHeight: 570
    standardButtons: Dialog.Close
    property var service: null
    function resultPath(job) {
        if (job.primary_output !== undefined) return job.primary_output || ""
        return Object.keys(job.outputs || {})[0] || ""
    }
    function activeStatus(status) { return ["queued", "running", "cancelling", "finalizing"].includes(status) }
    property var jobData: ({jobs: []})
    readonly property var notices: service && service.notifications.length ? JSON.parse(service.notifications) : []
    signal openDocument(string path)
    signal copyText(string text)
    initialFocusItem: previousRuns
    // The line that says what went wrong: the last "SomethingError: …" line of
    // a traceback, else the first line.
    function mainMessage(text) {
        const lines = (text || "").split("\n").map(line => line.trim()).filter(line => line.length)
        const errors = lines.filter(line => /^[A-Za-z_.]*(Error|Exception)\b/.test(line))
        return errors.length ? errors[errors.length - 1] : lines.length ? lines[0] : ""
    }
    function refreshJobs() { if (service) jobData = JSON.parse(service.jobs(previousRuns.checked)) }
    onOpened: refreshJobs()
    Timer { interval: 1500; repeat: true; running: dialog.visible; onTriggered: dialog.refreshJobs() }
    contentItem: ColumnLayout {
        AppCheckBox { id: previousRuns; text: "Include retained runs from earlier sessions"; onToggled: dialog.refreshJobs() }
        Label { text: dialog.jobData.error || (dialog.jobData.warnings || []).join("\n"); Layout.fillWidth: true; wrapMode: Text.Wrap; visible: text.length > 0 }
        Label { text: "No tasks or notifications this session."; visible: !(dialog.jobData.jobs || []).length && !dialog.notices.length }
        ScrollView {
            Layout.fillWidth: true; Layout.fillHeight: true; contentWidth: availableWidth
            ColumnLayout {
                width: parent.width
                Repeater {
                    model: dialog.jobData.jobs || []
                    delegate: ColumnLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        Label { text: modelData.operation + " — " + modelData.status; font.bold: true; Layout.fillWidth: true; wrapMode: Text.Wrap }
                        Label { text: modelData.source ? modelData.source.path : ""; Layout.fillWidth: true; elide: Text.ElideMiddle }
                        Label { text: modelData.diagnostics || ""; visible: text.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap }
                        ProgressBar { objectName: "taskProgress"; visible: dialog.activeStatus(modelData.status); indeterminate: true; Layout.fillWidth: true }
                        Flow {
                            Layout.fillWidth: true
                            Layout.preferredHeight: childrenRect.height
                            spacing: 6
                            AppButton { objectName: "cancelTask"; text: "Cancel"; visible: dialog.activeStatus(modelData.status); enabled: !!modelData.cancellable && ["queued", "running"].includes(modelData.status); onClicked: dialog.service.cancel_run(modelData.id) }
                            AppButton { text: "Open source"; onClicked: dialog.openDocument(modelData.source.path) }
                            AppButton { objectName: "copyTaskError"; text: "Copy error"; visible: dialog.mainMessage(modelData.diagnostics).length > 0; onClicked: dialog.copyText(dialog.mainMessage(modelData.diagnostics)) }
                            AppButton { text: "Copy diagnostics"; onClicked: dialog.copyText(JSON.stringify(modelData, null, 2)) }
                            AppButton { objectName: "openTaskResult"; text: "Open result"; visible: dialog.resultPath(modelData).length > 0; onClicked: dialog.openDocument(dialog.resultPath(modelData)) }
                        }
                    }
                }
                Repeater {
                    model: dialog.notices
                    delegate: ColumnLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        Label { text: modelData.time + " · " + modelData.kind; opacity: 0.7 }
                        TextArea { text: modelData.message; readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap; Layout.fillWidth: true; Accessible.name: "Notification" }
                        Flow {
                            Layout.fillWidth: true
                            Layout.preferredHeight: childrenRect.height
                            spacing: 6
                            AppButton { objectName: "copyNotice"; text: "Copy message"; onClicked: dialog.copyText(dialog.mainMessage(modelData.message)) }
                            AppButton { text: "Copy all"; visible: modelData.message.includes("\n"); onClicked: dialog.copyText(modelData.message) }
                            AppButton { text: "Open document"; visible: modelData.document.length > 0; onClicked: dialog.openDocument(modelData.document) }
                        }
                    }
                }
            }
        }
        AppButton { objectName: "clearTaskNotifications"; text: "Clear notifications"; onClicked: if (dialog.service) dialog.service.clear_notifications() }
    }
}
