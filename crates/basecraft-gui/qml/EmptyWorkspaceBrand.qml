import QtQuick
import QtQuick.Controls

// A decorative empty-state mark: never takes focus or pointer input.
Item {
    id: root
    property color backgroundColor: "white"
    readonly property bool darkBackground: 0.2126 * backgroundColor.r
        + 0.7152 * backgroundColor.g + 0.0722 * backgroundColor.b < 0.5
    opacity: 0.7

    Column {
        anchors.centerIn: parent
        width: Math.min(parent.width * 0.5, 800)
        spacing: 8

        Image {
            id: logo
            objectName: "emptyWorkspaceLogo"
            anchors.horizontalCenter: parent.horizontalCenter
            width: Math.min(root.width * 0.25, root.height * 1020 / 840)
            height: width * 420 / 1020
            source: Qt.resolvedUrl("../branding/wordmark-" + (root.darkBackground ? "dark" : "light") + ".png")
            fillMode: Image.PreserveAspectFit
            smooth: true
            mipmap: true
            Accessible.ignored: true
        }

        Label {
            objectName: "emptyWorkspaceSubtitle"
            width: parent.width
            text: "A workbench for evolutionary biology and taxonomy, bringing together hands-on analysis, automation and AI assistance, with provenance built in."
            color: root.darkBackground ? "#81978f" : "#83958b"
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }
    }
}
