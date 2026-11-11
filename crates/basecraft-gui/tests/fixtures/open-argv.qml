import QtQuick
import Basecraft
Main {
    id: test
    visible: true
    property int paintsWhileOpening: 0
    property bool waitingForOpen: true
    onFrameSwapped: { if (waitingForOpen) paintsWhileOpening += 1 }
    Rectangle { width: 8; height: 8; color: "red"; opacity: 0.5
        NumberAnimation on opacity { from: 0.1; to: 0.9; duration: 100; loops: Animation.Infinite }
    }
    property string folder: OPEN_TEST_FOLDER
    property var transitions: []
    Connections {
        target: test
        function onMetadataProjectChanged() { test.transitions.push(test.metadataProject) }
    }
    Timer {
        interval: 1
        repeat: true
        running: true
        onTriggered: {
            const a = test.documentAt(0)
            const b = test.documentAt(1)
            if (!a || !b) return
            if (JSON.stringify(test.transitions) !== JSON.stringify([test.folder + "/A", test.folder + "/B"])) {
                console.error("ARGV project callback order/count", JSON.stringify(test.transitions)); Qt.exit(2); return
            }
            if (test.metadataProject !== test.folder + "/B" || test.currentPath !== test.folder + "/B/alignment.afa"
                    || a.kind !== "sequence" || b.kind !== "alignment") {
                console.error("ARGV completion changed project or request-order tab selection"); Qt.exit(2); return
            }
            running = false
            console.log("ARGV mixed roots, metadata/submission legacy routes and late A completion passed")
            Qt.quit()
        }
    }
    Timer { interval: 15000; running: true; onTriggered: Qt.exit(3) }
}
