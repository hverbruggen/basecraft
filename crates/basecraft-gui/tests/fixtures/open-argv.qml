import QtQuick
import Basecraft
Main {
    id: test
    visible: false
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
