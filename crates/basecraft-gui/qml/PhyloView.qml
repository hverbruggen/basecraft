import QtQuick
import QtQuick.Controls.Basic
import Basecraft

// One open tree: its controller, the GPU view, and pointer and key
// handling. Window-level actions (menus, dialogs, the sidebar) are asked
// for through signals, as in the other editors.
FocusScope {
    id: doc
    property alias findBar: findBar
    function openFind() { findBar.openFind() }
    FindBar {
        id: findBar
        inputHint: "Taxon name"
        sequenceSearch: false
        nameSearch: true
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        controller: ctl
        documentFocusItem: view
        activeDocument: doc.visible
    }

    property bool overviewEnabled: true
    property var notificationSink: null
    Connections {
        target: ctl
        function onMessageChanged() {
            if (notificationSink && ctl.message.length)
                notificationSink.notification(ctl.file_path, ctl.message, ctl.message_kind)
        }
    }

    required property string path
    required property int index
    property alias controller: ctl
    property alias view: view
    readonly property bool dirty: ctl.dirty
    readonly property string kind: "tree"

    signal contextMenu(var info)
    signal toggleSidebar
    signal renameTip
    signal annotate
    signal openFile(string path)

    TreeController { id: ctl }
    Connections {
        target: ctl
        function onFileCreated(path) { doc.openFile(path) }
    }

    // Open once the view has its real size.
    Component.onCompleted: Qt.callLater(() => {
        ctl.resize(view.width, view.height)
        ctl.open_file(doc.path)
    })

    TreeCanvas {
        id: view
        anchors.fill: parent
        anchors.bottomMargin: findBar.height
        findBar: doc.findBar
        anchors.rightMargin: overviewWidth
        overviewEnabled: doc.overviewEnabled
        controller: ctl
        onContextMenu: (info) => doc.contextMenu(info)
        onToggleSidebar: doc.toggleSidebar()
        onRenameTip: doc.renameTip()
        onAnnotate: doc.annotate()
    }
}
