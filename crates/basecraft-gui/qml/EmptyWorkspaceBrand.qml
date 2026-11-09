import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Item {
    id: root
    property color backgroundColor: "white"
    property var recentProjects: []
    signal openProject()
    signal openRecent(string path)
    readonly property bool darkBackground: 0.2126 * backgroundColor.r
        + 0.7152 * backgroundColor.g + 0.0722 * backgroundColor.b < 0.5

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(root.width - 24, 420)
        height: Math.min(implicitHeight, root.height - 24)
        spacing: 12
        Image {
            objectName: "emptyWorkspaceLogo"
            Layout.alignment: Qt.AlignHCenter
            // Keep the mark modest relative to the main panel.
            Layout.preferredWidth: root.width * 0.25
            Layout.preferredHeight: width * 420 / 1020
            source: Qt.resolvedUrl("../branding/wordmark-" + (root.darkBackground ? "dark" : "light") + ".png")
            fillMode: Image.PreserveAspectFit
            smooth: true
            mipmap: true
            opacity: 0.7
            Accessible.role: Accessible.Graphic
            Accessible.name: "Basecraft"
        }
        EmptyView {
            Layout.fillWidth: true
            actionObjectName: "startOpenProject"
            actionText: "Open project directory"
            onActivated: root.openProject()
        }
        Label {
            text: "Recent projects"
            visible: root.recentProjects.length > 0
            Layout.alignment: Qt.AlignHCenter
        }
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: Math.min(projects.contentHeight, 180)
            visible: root.recentProjects.length > 0
            clip: true
            ListView {
                id: projects
                objectName: "startRecentProjects"
                model: root.recentProjects
                implicitHeight: contentHeight
                delegate: ItemDelegate {
                    required property string modelData
                    width: ListView.view.width
                    text: modelData
                    Accessible.name: "Open recent project " + modelData
                    onClicked: root.openRecent(modelData)
                    ToolTip.text: modelData
                    ToolTip.visible: hovered || activeFocus
                }
            }
        }
    }
}
