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
    signal detailsRequested()
    signal cancelRequested()
    readonly property string firstLine: message.split("\n")[0]
    visible: busy || message.length > 0
    BusyIndicator { running: bar.busy; visible: running; implicitWidth: 24; implicitHeight: 24 }
    AppErrorLabel { objectName: "messageText"; Layout.fillWidth: true; maximumLineCount: 1; elide: Text.ElideRight; visible: bar.kind === "error"; text: bar.firstLine }
    Label { Layout.fillWidth: true; maximumLineCount: 1; elide: Text.ElideRight; visible: bar.kind !== "error"; text: bar.firstLine }
    AppButton { objectName: "messageDetails"; text: "Details"; padding: 6; visible: bar.kind === "error" && bar.details.length > 0; onClicked: bar.detailsRequested() }
    AppButton { text: "Cancel analysis"; padding: 6; visible: bar.busy; onClicked: bar.cancelRequested() }
}
