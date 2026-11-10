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
    function check(ok, message) {
        if (!ok) { console.error("OPEN_CONSENT " + scenario + ": " + message); Qt.exit(2); throw new Error(message) }
    }
    function dialog(name) {
        for (const item of test.contentData)
            if (item && item.objectName === name) return item
        check(false, "missing dialog " + name)
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
            check(targetIndex({token: ownerB.token, controller: c, path: ownerB.path}) === -1, "mismatched owner accepted")
            edit(b, "G"); pageB = b.accessible_page(0)
            if (scenario.includes("quit")) {
                if (scenario === "quit-batch") {
                    middle = documentAt(2).controller
                    edit(middle, "T"); middlePage = middle.accessible_page(0)
                }
                consent = dialog("quitDocumentsDialog"); quitConsent = consent
                consent.ask(unsavedTabs()) // B (and batch C); the unrelated owner becomes dirty later.
                check(consent.unsaved.length === (scenario === "quit-batch" ? 2 : 1) && consent.unsaved[0].controller === b, "quit snapshot did not capture its owners")
            }
            edit(c, "T"); pageC = c.accessible_page(0)
            recordC = c.document_record(); revisionC = c.record_revision()
            c.find_query("T", false)
            if (scenario === "quit-batch") {
                quitConsent.accepted()
                consent = dialog("evidenceSaveDialog")
                check(consent.visible && pendingEvidenceSave.owner.controller === b, "quit did not pause for B evidence")
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
                saveDocument(1, folder + "/chosen.gb", success => { check(success, "evidence save failed"); completedSave = true })
                consent = dialog("evidenceSaveDialog")
                check(consent.visible && pendingEvidenceSave.owner.controller === b, "real evidence consent missing")
            }
            return
        }
        if (stage === 2 && targetIndex(ownerB) === 0) {
            stage = 3
            check(targetIndex(ownerC) === (scenario === "quit-batch" ? 2 : 1), "unrelated owner did not shift after failed A")
            if (scenario.includes("stale")) {
                removeDocument(0)
                check(targetIndex(ownerB) === -1 && targetIndex(ownerC) === 0, "retired owner was not rejected")
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
    function verify() {
        const ci = targetIndex(ownerC)
        check(ci >= 0 && documentAt(ci).controller === c && c.dirty, "consent discarded or saved dirty C")
        check(c.accessible_page(0) === pageC && c.document_record() === recordC && c.record_revision() === revisionC, "consent changed C contents/records/revision")
        check(!closeConfirmed, "old quit consent covered newly dirty C")
        if (scenario.includes("stale")) {
            if (scenario.includes("evidence")) check(!completedSave && pendingEvidenceSave === null, "retired evidence owner executed")
        } else if (scenario.includes("close")) {
            check(targetIndex(ownerB) === -1 && ci === 0, "close did not remove only B")
            if (scenario.includes("save")) {
                check(verification.open_file(folder + "/B.gb"), "B save missing")
                check(verification.accessible_page(0) === pageB, "saved wrong B bytes")
            }
        } else if (scenario.includes("saveas") || scenario.includes("evidence")) {
            check(b.file_path === folder + "/chosen.gb" && !b.dirty, "chosen path did not belong to B")
            check(documentAt(0).path === folder + "/chosen.gb" && documentAt(1).path === folder + "/C.gb", "wrong list path association")
            check(verification.open_file(folder + "/chosen.gb"), "chosen file missing")
            check(verification.accessible_page(0) === pageB, "chosen bytes not B")
            openPath(folder + "/chosen.gb")
            check(current === b && documentAt(2) === null, "path_changed associated wrong token")
        } else if (scenario.includes("quit")) {
            check(quitConsent.visible && quitConsent.unsaved.some(owner => owner.controller === c), "quit did not request fresh consent for unrelated dirty owner")
            if (scenario.includes("save") || scenario === "quit-batch") {
                check(!b.dirty && verification.open_file(folder + "/B.gb"), "quit did not save B")
                check(verification.accessible_page(0) === pageB, "quit saved wrong bytes")
                if (scenario === "quit-batch") {
                    check(!middle.dirty && documentAt(1).controller === middle, "quit continuation lost second captured owner")
                    check(verification.open_file(folder + "/C.gb") && verification.accessible_page(0) === middlePage, "quit second save used wrong bytes")
                }
            } else check(b.dirty, "quit discard unexpectedly saved B")
        }
        // Finish the existing asynchronous Find without changing its owner.
        const found = c.find_poll(0)
        if (!found.length && c.overview_matches === "[]" || found.length && JSON.parse(found).busy) { Qt.callLater(verify); return }
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
