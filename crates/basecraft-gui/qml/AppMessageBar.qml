import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// One line at most; the full diagnostics stay behind Details.
RowLayout {
    id: bar
    property string message: ""
    property string kind: ""
    property string details: ""
    property bool busy: false
    // The caller supplies the real stage when one is available. Never infer
    // progress or completion from elapsed time.
    property string workingText: "Working"
    property bool cancellable: true
    property string cancelText: "Cancel analysis"
    signal detailsRequested()
    signal cancelRequested()
    readonly property string firstLine: (busy ? workingText : message).split("\n")[0]
    visible: busy || message.length > 0
    Accessible.role: Accessible.StaticText
    Accessible.name: firstLine
    BusyIndicator { objectName: "activitySpinner"; running: bar.busy && bar.visible; visible: bar.busy; implicitWidth: 24; implicitHeight: 24; Accessible.ignored: true }
    AppErrorLabel { objectName: "messageText"; Layout.fillWidth: true; maximumLineCount: 1; elide: Text.ElideRight; visible: !bar.busy && (bar.kind === "error" || bar.kind === "fail"); text: bar.firstLine }
    Label { objectName: "activityText"; Layout.fillWidth: true; maximumLineCount: 1; elide: Text.ElideRight; visible: bar.busy || (bar.kind !== "error" && bar.kind !== "fail"); text: bar.firstLine }
    AppButton { objectName: "messageDetails"; text: "Details"; padding: 6; visible: !bar.busy && (bar.kind === "error" || bar.kind === "fail") && bar.details.length > 0; onClicked: bar.detailsRequested() }
    AppButton { objectName: "activityCancel"; text: bar.cancelText; padding: 6; visible: bar.busy && bar.cancellable; onClicked: bar.cancelRequested() }
}
