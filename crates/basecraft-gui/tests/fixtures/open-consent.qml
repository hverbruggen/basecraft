import QtQuick
import Basecraft
Main {
    id: test
    visible: false
    property string folder: OPEN_TEST_FOLDER
    property string scenario: OPEN_TEST_SCENARIO
    property int stage: 0
    property var b: null
    property var c: null
    property var ownerB: null
    property var ownerC: null
    property string pageB: ""
    property string pageC: ""
    property string recordC: ""
    property string revisionC: ""
    property var consent: null
    property var quitConsent: null
    property var middle: null
    property string middlePage: ""
    property bool completed: false
    GenomeController { id: verification }
    property var verifiedFiles: ({})
    function checkFile(path, page) {
        if (verifiedFiles[path] !== undefined) {
            check(verifiedFiles[path] === page, "saved wrong bytes at " + path)
            return true
        }
        const opened = function(ok) {
            verification.opened.disconnect(opened)
            check(ok, "saved file could not open: " + path)
            verifiedFiles[path] = verification.accessible_page(0)
            Qt.callLater(verify)
        }
        verification.opened.connect(opened)
        check(verification.open_file(path), "saved file open rejected: " + path)
        return false
    }
    function check(ok, message) {
        if (!ok) { console.error("OPEN_CONSENT " + scenario + ": " + message); Qt.exit(2); throw new Error(message) }
    }
    // Main's own objects, which a derived window reaches only by walking them.
    function child(found) {
        for (const item of test.contentData) {
            if (!item) continue
            if (found(item)) return item
            if (item.item && found(item.item)) return item.item
            const inner = item.data
            for (let i = 0; inner && i < inner.length; i++) if (inner[i] && found(inner[i])) return inner[i]
        }
        return null
    }
    function opens() { return child(item => typeof item.target_index === "function") }
    function evidence() { return child(item => item.dialogName === "evidenceSaveDialog") }
    function dialog(name) {
        const found = child(item => item.objectName === name)
        check(found, "missing dialog " + name)
        return found
    }
    function edit(controller, base) {
        controller.key(Qt.Key_A, "", Qt.ControlModifier)
        controller.key(base === "G" ? Qt.Key_G : Qt.Key_T, base, 0)
        check(controller.dirty, "fixture edit must be dirty")
    }
    function advance() {
        if (completed) return
        if (stage === 0 && documentAt(1)) {
            stage = 1
            openPath(folder + "/C.gb")
        }
        if (stage === 1 && documentAt(2) && scenario === "quit-batch") {
            stage = 11
            openPath(folder + "/D.gb")
            return
        }
        if ((stage === 1 && documentAt(1) && documentAt(2)) || (stage === 11 && documentAt(3))) {
            stage = 2
            check(documentAt(0) === null, "A must still be pending when consent opens")
            b = documentAt(1).controller; c = documentAt(scenario === "quit-batch" ? 3 : 2).controller
            ownerB = documentTarget(1); ownerC = documentTarget(scenario === "quit-batch" ? 3 : 2)
            edit(b, "G"); pageB = b.accessible_page(0)
            if (scenario.includes("quit")) {
                if (scenario === "quit-batch") {
                    middle = documentAt(2).controller
                    edit(middle, "T"); middlePage = middle.accessible_page(0)
                }
                // B (and batch C); the unrelated owner becomes dirty later.
                quitStep(JSON.parse(opens().quit()))
                consent = dialog("quitDocumentsDialog"); quitConsent = consent
                check(consent.unsaved.length === (scenario === "quit-batch" ? 2 : 1) && consent.unsaved[0].token === ownerB.token, "quit snapshot did not capture its owners")
            }
            edit(c, "T"); pageC = c.accessible_page(0)
            recordC = c.document_record(); revisionC = c.record_revision()
            c.find_query("T", false)
            if (scenario === "quit-batch") {
                quitConsent.accepted()
                consent = dialog("evidenceSaveDialog")
                check(consent.visible && evidence().request.owner.controller === b, "quit did not pause for B evidence")
            }
            if (scenario.includes("close")) {
                closeTab(1)
                consent = dialog("closeDocumentDialog")
                check(consent.target.controller === b && consent.visible, "real close consent missing")
            } else if (scenario.includes("saveas")) {
                openPath(folder + "/B.gb")
                saveAs()
                consent = dialog("saveDocumentDialog")
                check(consent.target.controller === b, "Save As captured wrong owner")
                consent.selectedFile = "file://" + folder + "/chosen.gb"
            } else if (scenario.includes("evidence")) {
                // A retired owner ends its save unsuccessfully; the others must succeed.
                saveTarget(documentTarget(1), folder + "/chosen.gb", success => {
                    check(success || scenario.includes("stale"), "evidence save failed"); saveEnded = true; completedSave = success })
                consent = dialog("evidenceSaveDialog")
                check(consent.visible && evidence().request.owner.controller === b, "real evidence consent missing")
            }
            return
        }
        if (stage === 2 && opens().target_index(ownerB.token, ownerB.path) === 0) {
            stage = 3
            check(opens().target_index(ownerC.token, ownerC.path) === (scenario === "quit-batch" ? 2 : 1), "unrelated owner did not shift after failed A")
            if (scenario.includes("stale")) {
                removeDocument(0)
                check(opens().target_index(ownerB.token, ownerB.path) === -1 && opens().target_index(ownerC.token, ownerC.path) === 0, "retired owner was not rejected")
            }
            if (scenario === "quit-batch") consent.choice("preserve_copy")
            else if (scenario.includes("close-discard")) consent.discarded()
            else if (scenario.includes("close-save")) consent.accepted()
            else if (scenario.includes("saveas")) consent.accepted()
            else if (scenario.includes("evidence")) {
                if (scenario.includes("stale")) consent.relinkOriginal(folder + "/B.gb")
                consent.choice("preserve_copy")
            }
            else if (scenario.includes("quit-discard")) consent.discarded()
            else if (scenario.includes("quit-save")) consent.accepted()
            Qt.callLater(verify)
        }
    }
    property bool completedSave: false
    property bool saveEnded: false
    function verify() {
        if (b && b.busy) {
            const ready = function(ok) { b.opened.disconnect(ready); check(ok, "Save As reopen failed"); verify() }
            b.opened.connect(ready)
            return
        }
        const ci = opens().target_index(ownerC.token, ownerC.path)
        check(ci >= 0 && documentAt(ci).controller === c && c.dirty, "consent discarded or saved dirty C")
        check(c.accessible_page(0) === pageC && c.document_record() === recordC && c.record_revision() === revisionC, "consent changed C contents/records/revision")
        check(!closeConfirmed, "old quit consent covered newly dirty C")
        if (scenario.includes("stale")) {
            // The retired owner's save ends, unsuccessfully, and its dialog closes.
            if (scenario.includes("evidence")) check(saveEnded && !completedSave && !consent.visible, "retired evidence owner executed or kept its dialog")
        } else if (scenario.includes("close")) {
            check(opens().target_index(ownerB.token, ownerB.path) === -1 && ci === 0, "close did not remove only B")
            if (scenario.includes("save")) {
                if (!checkFile(folder + "/B.gb", pageB)) return
            }
        } else if (scenario.includes("saveas") || scenario.includes("evidence")) {
            check(b.file_path === folder + "/chosen.gb" && !b.dirty, "chosen path did not belong to B")
            check(documentAt(0).path === folder + "/chosen.gb" && documentAt(1).path === folder + "/C.gb", "wrong list path association")
            if (!checkFile(folder + "/chosen.gb", pageB)) return
            openPath(folder + "/chosen.gb")
            check(current === b && documentAt(2) === null, "path_changed associated wrong token")
        } else if (scenario.includes("quit")) {
            check(quitConsent.visible && quitConsent.unsaved.some(owner => owner.token === ownerC.token), "quit did not request fresh consent for unrelated dirty owner")
            if (scenario.includes("save") || scenario === "quit-batch") {
                check(!b.dirty, "quit did not save B")
                if (!checkFile(folder + "/B.gb", pageB)) return
                if (scenario === "quit-batch") {
                    check(!middle.dirty && documentAt(1).controller === middle, "quit continuation lost second captured owner")
                    if (!checkFile(folder + "/C.gb", middlePage)) return
                }
            } else check(b.dirty, "quit discard unexpectedly saved B")
        }
        // Finish the existing asynchronous Find without changing its owner.
        c.find_step(0)
        if (c.overview_matches === "[]" || c.find_status === "Finding…") { Qt.callLater(verify); return }
        check(c.overview_matches !== "[]", "C Find results were lost")
        c.undo(); check(c.accessible_page(0) !== pageC, "C undo was lost")
        c.redo(); check(c.accessible_page(0) === pageC && c.dirty, "C redo was lost")
        completed = true
        consent.close()
        console.log("OPEN_CONSENT passed: " + scenario)
        Qt.quit()
    }
    onCurrentChanged: advance()
    onCurrentPathChanged: advance()
    Component.onCompleted: Qt.callLater(() => { openPath(folder + "/missing.gb"); openPath(folder + "/B.gb") })
    Timer { interval: 1; repeat: true; running: !test.completed; onTriggered: test.advance() }
    Timer { interval: 15000; running: true; onTriggered: { console.error("OPEN_CONSENT timeout " + scenario + ": " + stage); Qt.exit(3) } }
}
