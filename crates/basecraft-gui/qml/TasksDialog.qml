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
    property var jobData: ({jobs: []})
    readonly property var notices: service && service.notifications.length ? JSON.parse(service.notifications) : []
    signal openDocument(string path)
    signal copyText(string text)
    initialFocusItem: previousRuns
    function refreshJobs() { if (service) jobData = JSON.parse(service.jobs(previousRuns.checked)) }
    onOpened: refreshJobs()
    Timer { interval: 1500; repeat: true; running: dialog.visible; onTriggered: dialog.refreshJobs() }
    contentItem: ColumnLayout {
        CheckBox { id: previousRuns; text: "Include retained runs from earlier sessions"; onToggled: dialog.refreshJobs() }
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
                        ProgressBar { visible: modelData.status === "running"; indeterminate: true; Layout.fillWidth: true }
                        Flow {
                            Layout.fillWidth: true
                            Layout.preferredHeight: childrenRect.height
                            spacing: 6
                            AppButton { text: "Cancel"; visible: modelData.status === "running"; enabled: !!modelData.cancellable; onClicked: dialog.service.cancel_run(modelData.id) }
                            AppButton { text: "Open source"; onClicked: dialog.openDocument(modelData.source.path) }
                            AppButton { text: "Copy diagnostics"; onClicked: dialog.copyText(JSON.stringify(modelData, null, 2)) }
                            AppButton { text: "Open result"; visible: Object.keys(modelData.outputs || {}).length > 0; onClicked: dialog.openDocument(Object.keys(modelData.outputs)[0]) }
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
                        AppButton { text: "Open document"; visible: modelData.document.length > 0; onClicked: dialog.openDocument(modelData.document) }
                    }
                }
            }
        }
        AppButton { text: "Clear notifications"; onClicked: if (dialog.service) dialog.service.clear_notifications() }
    }
}
