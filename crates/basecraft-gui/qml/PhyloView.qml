import QtQuick
import QtQuick.Controls.Basic
import Basecraft

// One open tree: its controller, the GPU view, and pointer and key
// handling. Window-level actions (menus, dialogs, the sidebar) are asked
// for through signals, as in the other editors.
FocusScope {
    id: doc
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

    // Lines snap to whole screen pixels.
    readonly property real pixelRatio: view.Window.window ? view.Window.window.devicePixelRatio : 1
    onPixelRatioChanged: ctl.set_pixel_ratio(pixelRatio)

    // Open once the view has its real size.
    Component.onCompleted: Qt.callLater(() => {
        ctl.set_pixel_ratio(doc.pixelRatio)
        ctl.resize(view.width, view.height)
        ctl.open_file(doc.path)
    })

    TreeCanvas {
        id: view
        anchors.fill: parent
        controller: ctl
        onContextMenu: (info) => doc.contextMenu(info)
        onToggleSidebar: doc.toggleSidebar()
        onRenameTip: doc.renameTip()
        onAnnotate: doc.annotate()
    }
}
