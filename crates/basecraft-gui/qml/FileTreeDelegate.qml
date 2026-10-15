import QtQuick
import QtQuick.Controls.Basic

TreeViewDelegate {
    id: fileDelegate
    property string currentPath: ""
    property color foreground: palette.text
    signal openRequested(string filePath)
    implicitWidth: treeView.width
    // Model roles refresh when rows are sorted, inserted, renamed or reused.
    // An invokable index lookup in a binding cannot track those changes.
    readonly property string path: model.filePath
    highlighted: path === currentPath
    implicitHeight: Math.max(24, contentItem.implicitHeight + topPadding + bottomPadding)
    topPadding: 0
    bottomPadding: 0
    indicator: AppIcon {
        x: fileDelegate.leftMargin + fileDelegate.depth * fileDelegate.indentation
        y: (fileDelegate.height - height) / 2
        width: 10
        height: 10
        name: fileDelegate.expanded ? "down" : "right"
        size: 10
        visible: fileDelegate.isTreeNode && fileDelegate.hasChildren
    }
    contentItem: Label {
        text: fileDelegate.model.display
        color: fileDelegate.highlighted ? fileDelegate.palette.highlightedText : fileDelegate.palette.text
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    background: Rectangle {
        color: fileDelegate.highlighted ? fileDelegate.palette.highlight
            : Qt.alpha(foreground, fileDelegate.hovered ? 0.06 : fileDelegate.row % 2 ? 0.025 : 0)
    }
    onClicked: {
        if (treeView.model.isDirAt(treeView.index(row, column))) treeView.toggleExpanded(row)
        else openRequested(path)
    }
}
