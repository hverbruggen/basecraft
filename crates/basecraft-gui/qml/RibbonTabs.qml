import QtQuick
import QtQuick.Controls.Basic

TabBar {
    id: tabs
    property var names: []
    property string selectedName
    signal selected(string name)
    currentIndex: names.indexOf(selectedName)
    property bool rebuilding: false
    // Repeater briefly resets currentIndex while contextual tabs are replaced.
    // Do not publish that transient index as a user selection.
    onNamesChanged: {
        rebuilding = true
        Qt.callLater(() => {
            currentIndex = names.indexOf(selectedName)
            rebuilding = false
        })
    }
    onSelectedNameChanged: currentIndex = names.indexOf(selectedName)
    onCurrentIndexChanged: Qt.callLater(() => {
        if (!rebuilding && currentIndex >= 0 && currentIndex < names.length)
            selected(names[currentIndex])
    })
    background: Item {}
    Repeater {
        model: tabs.names
        AppTabButton {
            id: tab
            required property string modelData
            width: implicitWidth
            text: modelData
            focusPolicy: Qt.TabFocus
            topPadding: 3
            bottomPadding: 3
            leftPadding: 12
            rightPadding: 12
            background: Rectangle {
                color: Qt.alpha(tab.palette.windowText, tab.checked ? 0.07 : tab.hovered ? 0.04 : 0)
                border.color: tab.visualFocus ? tab.palette.highlight : "transparent"
                Rectangle {
                    anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                    height: 2
                    visible: tab.checked
                    color: tab.palette.highlight
                }
            }
        }
    }
}
