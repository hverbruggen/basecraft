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
    // The window's project is B, chosen before Main opens its arguments
    // (bindings run before Component.onCompleted).
    readonly property bool projectChosen: { chooseProject(folder + "/B"); return true }
    Timer {
        interval: 1
        repeat: true
        running: true
        onTriggered: {
            const a = test.documentAt(0)
            const b = test.documentAt(1)
            if (!a || !b) return
            // Opening files never changes the chosen project (D10).
            if (test.metadataProject !== test.folder + "/B" || test.currentPath !== test.folder + "/B/alignment.afa"
                    || a.kind !== "sequence" || b.kind !== "alignment") {
                console.error("ARGV changed the chosen project or the request-order tab selection"); Qt.exit(2); return
            }
            running = false
            console.log("ARGV files from two folders, the project's metadata and submission, and a late first file passed")
            Qt.quit()
        }
    }
    Timer { interval: 15000; running: true; onTriggered: Qt.exit(3) }
}
