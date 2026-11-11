import QtQuick
import Basecraft
Main {
    id: test
    visible: false
    property string folder: OPEN_TEST_FOLDER
    property string scenario: OPEN_TEST_SCENARIO
    property bool began: false
    property bool failureInjected: false
    function pendingCanvas(item) {
        if (item.adoptedSlot === true && item.path === folder + "/sequence.gb") return item
        for (const child of item.children || []) { const found = pendingCanvas(child); if (found) return found }
        return null
    }
    property int stage: 0
    property var original: null
    property string originalPage: ""
    function recordDialog() {
        for (const item of test.contentData)
            if (item && item.objectName === "documentRecordDialog") return item
        return null
    }
    function check(ok, message) {
        if (!ok) { console.error("OPEN_LIFECYCLE " + scenario + ": " + message); Qt.exit(2) }
    }
    function finish() { began = false; Qt.callLater(() => { console.log("OPEN_LIFECYCLE passed: " + scenario); Qt.quit() }) }
    function advance() {
        if (!began) return
        if (scenario === "fail-construction" && !failureInjected) {
            const slot = pendingCanvas(test.contentItem)
            if (!slot) return
            failureInjected = true
            slot.constructionFailed()
            return
        }
        const first = documentAt(0)
        const second = documentAt(1)
        if (scenario === "legacy") {
            if (stage === 0 && currentKind === "delimitation") {
                stage = 1
                openPath(folder + "/trace.submission.json")
            } else if (stage === 1 && currentKind === "trace" && current.review_mode) {
                current.cancel_calls()
                check(currentPath === folder + "/trace.submission.json", "JSON trace lost submission-name precedence")
                stage = 2
                openPath(folder + "/trace.submission.json", true)
            } else if (stage === 2) {
                const dialog = recordDialog()
                if (!dialog || !dialog.visible) return
                check(dialog.controller === current && dialog.page === 2, "legacy authored continuation lost owner")
                dialog.close()
                stage = 3
                openPath(folder + "/trace.ab1")
            } else if (stage === 3 && currentKind === "trace" && current.review_mode && currentPath === folder + "/trace.ab1") {
                current.cancel_calls()
                stage = 4
                openPath(folder + "/unknown.data")
            } else if (stage === 4 && currentKind === "sequence" && currentPath === folder + "/unknown.data") {
                check(!current.file_path.length, "unknown-format legacy refusal changed")
                finish()
            }
        } else if (scenario.startsWith("fail-")) {
            if (!first || currentPath !== folder + "/tree.nwk") return
            check(second === null, "failed request retained a pending slot")
            check(first.controller === current && !current.dirty, "failure changed surviving document")
            finish()
        } else if (scenario.startsWith("review-")) {
            const dialog = recordDialog()
            check(dialog !== null, "real document record dialog not found")
            if (scenario === "review-supersede") {
                if (!first || !second || currentPath !== folder + "/tree.nwk") return
                check(!dialog.visible, "superseded review opened another document draft")
                finish()
            } else if (scenario === "review-cancel") {
                if (!first || currentPath !== folder + "/tree.nwk") return
                check(!dialog.visible, "cancelled review opened tree draft")
                finish()
            } else if (scenario === "review-fail") {
                if (!first || currentPath !== folder + "/tree.nwk") return
                check(!dialog.visible, "failed review opened tree draft")
                finish()
            } else {
                if (!first || currentPath !== folder + "/sequence.gb") return
                if (scenario === "review-duplicate" && stage === 0) {
                    stage = 1
                    check(!dialog.visible, "ordinary open showed record dialog")
                    openPath(folder + "/sequence.gb", true)
                }
                check(dialog.visible && dialog.page === 2, "review did not show authored draft page")
                check(dialog.controller === current && dialog.documentPath === currentPath, "review has wrong document")
                dialog.draftText = "Keep this authored draft"
                dialog.rememberDraft()
                dialog.close()
                check(dialog.unsavedDrafts().length === 1, "authored text discarded")
                finish()
            }
        } else if (scenario === "reorder") {
            if (!first || !second || currentPath !== folder + "/tree.nwk") return
            check(first.kind === "sequence" && second.kind === "tree", "request order changed")
            check(current === second.controller, "late A stole B activation")
            finish()
        } else if (scenario === "cancel-reopen") {
            if (!first || currentPath !== folder + "/sequence.gb") return
            check(second === null, "cancelled lifetime recreated a tab")
            check(first.controller === current, "reopen has wrong owner")
            finish()
        } else if (scenario === "save-as") {
            if (stage === 0 && first && currentPath === folder + "/sequence.gb") {
                stage = 1
                original = current
                originalPage = current.accessible_page(0)
                current.key(Qt.Key_A, "", Qt.ControlModifier)
                current.key(Qt.Key_G, "G", 0)
                check(current.dirty, "edit not dirty")
                current.undo()
                check(current.accessible_page(0) === originalPage, "undo lost sequence/annotations")
                current.redo()
                check(current.dirty, "redo lost edit")
                current.undo()
                saveTarget(documentTarget(0), folder + "/saved.gb", success => {
                    check(success, "Save As failed")
                    check(first.controller === original && !original.dirty, "Save As replaced controller")
                    openPath(folder + "/saved.gb")
                    check(documentAt(1) === null && current === original, "saved path duplicate acquired again")
                    stage = 2
                    openPath(folder + "/sequence.gb")
                })
            } else if (stage === 2 && second && currentPath === folder + "/sequence.gb") {
                stage = 3
                check(current !== original, "old path activated renamed document")
                check(current.accessible_page(0) === originalPage, "original source changed")
                closeTab(1)
                openPath(folder + "/saved.gb")
                check(current === original && documentAt(1) === null, "ready owner lost after closing sibling")
                finish()
            }
        }
    }
    onCurrentChanged: Qt.callLater(advance)
    onCurrentPathChanged: Qt.callLater(advance)
    function begin() {
        began = true
        if (scenario === "legacy") { openPath(folder + "/hypotheses.spart"); return }
        if (scenario === "review-fail") openPath(folder + "/missing.gb", true)
        else openPath(folder + "/sequence.gb", scenario.startsWith("review-") && scenario !== "review-duplicate")
        if (scenario === "review-cancel") { closeTab(0); openPath(folder + "/tree.nwk") }
        if (scenario === "review-supersede" || scenario === "review-fail") openPath(folder + "/tree.nwk")
        if (scenario === "reorder" || scenario.startsWith("fail-")) openPath(folder + "/tree.nwk")
        if (scenario === "cancel-reopen") { closeTab(0); openPath(folder + "/sequence.gb") }
    }
    Component.onCompleted: { if (scenario === "fail-construction") begin(); else Qt.callLater(begin) }
    Timer { interval: 1; repeat: true; running: test.began; onTriggered: test.advance() }
    Timer { interval: 15000; running: true; onTriggered: { console.error("OPEN_LIFECYCLE timeout " + scenario + ": " + stage); Qt.exit(3) } }
}
