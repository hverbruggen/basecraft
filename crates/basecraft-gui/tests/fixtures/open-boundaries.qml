import QtQuick
import Basecraft
Main {
    id: test
    visible: false
    property string folder: OPEN_TEST_FOLDER
    property string scenario: OPEN_TEST_SCENARIO
    property int stage: 0
    property var original: null
    property string retiredToken: ""
    property bool injected: false
    property bool loaderError: false
    function check(ok, message) {
        if (!ok) { console.error("OPEN_BOUNDARY " + scenario + ": " + message); Qt.exit(2); throw new Error(message) }
    }
    function slotAt(item, path) {
        if (item.adoptedSlot === true && item.path === path) return item
        for (const child of item.children || []) { const found = slotAt(child, path); if (found) return found }
        return null
    }
    function loaderAt(item) {
        for (const child of item.children || [])
            if (typeof child.setSource === "function") return child
        return null
    }
    function inject(slot) {
        // This callback was scheduled by admittedControllerChanged BEFORE the
        // real view's deferred fit/ack callback. Installation has now returned.
        check(slot.admittedController && slot.admittedController.file_path === folder + "/sequence.gb", "owner not installed")
        check(!slot.ready && documentAt(1) === null, "test missed constructing-before-ack boundary")
        injected = true
        retiredToken = slot.openToken
        if (scenario === "close-before-ack") closeTab(1)
        else {
            const loader = loaderAt(slot)
            check(loader !== null, "real canvas Loader not found")
            loader.setSource("qrc:/basecraft-test-missing-view.qml")
            check(loader.status === Loader.Error, "actual Loader.Error was not raised")
            loaderError = true
        }
        Qt.callLater(advance)
    }
    function advance() {
        if (scenario.startsWith("double-queue")) {
            if (stage === 0) {
                stage = 1
                const pending = slotAt(contentItem, folder + "/sequence.gb")
                check(pending && !pending.admittedController && !pending.ready, "double failed queue installed a candidate")
                retiredToken = pending.openToken
                if (scenario === "double-queue-close") closeTab(0)
                // Next request cleans up a cancelled lifetime even without a delivered error.
                openPath(folder + "/sequence.gb")
                return
            }
            if (stage === 1 && documentAt(0)) {
                check(currentPath === folder + "/sequence.gb" && documentAt(1) === null, "queue recovery kept stale row")
                check(documentTarget(0).token !== retiredToken, "queue recovery reused cancelled token")
                finish()
            }
            return
        }
        if (stage === 0 && documentAt(0)) {
            stage = 1
            original = documentAt(0).controller
            openPath(folder + "/sequence.gb")
            const slot = slotAt(contentItem, folder + "/sequence.gb")
            check(slot !== null, "pending slot not found")
            slot.admittedControllerChanged.connect(() => { if (!injected) Qt.callLater(() => inject(slot)) })
        } else if (stage === 1 && injected && requestIndex(retiredToken) < 0) {
            stage = 2
            Qt.callLater(() => {
                check(documentAt(0).controller === original && documentAt(1) === null, "late ack changed surviving owner or resurrected slot")
                if (scenario === "loader-error") check(loaderError, "Loader.Error callback was bypassed")
                finish()
            })
        }
    }
    function finish() { stage = 9; console.log("OPEN_BOUNDARY passed: " + scenario); Qt.quit() }
    onCurrentPathChanged: Qt.callLater(advance)
    Component.onCompleted: {
        if (scenario.startsWith("double-queue")) {
            // Rust releases/waits for this worker after load_data and BEFORE exec.
            openPath(folder + "/sequence.gb")
            Qt.callLater(advance)
        } else Qt.callLater(() => openPath(folder + "/tree.nwk"))
    }
    Timer { interval: 1; repeat: true; running: stage < 9; onTriggered: test.advance() }
    Timer { interval: 15000; running: true; onTriggered: { console.error("OPEN_BOUNDARY timeout " + scenario + ": " + stage); Qt.exit(3) } }
}
