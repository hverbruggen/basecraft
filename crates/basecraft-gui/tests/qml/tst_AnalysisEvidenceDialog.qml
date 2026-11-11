import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

TestCase {
    id: testCase
    name: "AnalysisEvidenceDialog"
    visible: true
    width: 680
    height: 700
    when: windowShown
    Component { id: dialogComponent; AnalysisEvidenceDialog {} }
    SignalSpy { id: choiceSpy; signalName: "choice" }
    SignalSpy { id: rejectSpy; signalName: "rejected" }
    function clickButton(button) { waitForRendering(button); mouseClick(button, button.width / 2, button.height / 2) }
    function test_choices_and_escape() {
        const dialog = createTemporaryObject(dialogComponent, testCase)
        dialog.conflict = {analyses: [{result: "genes.mafft.fasta"}, {result: "genes.nwk"}], options: ["cancel", "preserve_copy", "remove_analysis", "break_evidence"]}
        choiceSpy.target = dialog
        rejectSpy.target = dialog
        dialog.open()
        tryCompare(dialog, "opened", true)
        const preserve = findChild(dialog, "preserveInput")
        tryCompare(preserve, "visible", true)
        verify(preserve.enabled)
        clickButton(preserve)
        compare(choiceSpy.signalArguments[0][0], "preserve_copy")
        clickButton(findChild(dialog, "removeAnalysis"))
        compare(choiceSpy.signalArguments[1][0], "remove_analysis")
        clickButton(findChild(dialog, "breakEvidence"))
        compare(choiceSpy.signalArguments[2][0], "break_evidence")
        dialog.conflict = {analyses: [{result: "missing.nwk"}], options: ["cancel", "remove_analysis", "break_evidence"]}
        verify(!preserve.enabled)
        dialog.forceActiveFocus()
        keyClick(Qt.Key_Escape)
        tryCompare(dialog, "visible", false)
        compare(rejectSpy.count, 1)
        compare(choiceSpy.count, 3)
    }
    function test_incomplete_defaults_to_cancel_and_acknowledgement_is_one_check() {
        const dialog = createTemporaryObject(dialogComponent, testCase)
        dialog.check = {token: "first", evidence: {issues: [{location: "analysis", reason: "unreadable ledger"}], conflict: {analyses: [], options: ["cancel"]}}}
        choiceSpy.target = dialog
        choiceSpy.clear()
        rejectSpy.target = dialog
        rejectSpy.clear()
        dialog.open()
        tryCompare(dialog, "opened", true)
        const acknowledgement = findChild(dialog, "acknowledgeIncompleteEvidence")
        const unchecked = findChild(dialog, "saveUnchecked")
        verify(!acknowledgement.checked)
        verify(!unchecked.enabled)
        tryCompare(dialog.standardButton(Dialog.Cancel), "activeFocus", true)
        keyClick(Qt.Key_Return)
        tryCompare(dialog, "visible", false)
        compare(rejectSpy.count, 1)
        compare(choiceSpy.count, 0)
        dialog.open()
        tryCompare(dialog, "opened", true)
        clickButton(acknowledgement)
        verify(unchecked.enabled)
        clickButton(unchecked)
        compare(choiceSpy.signalArguments[0][0], "cancel")
        dialog.check = {token: "second", evidence: {issues: [{location: "analysis", reason: "changed problem"}], conflict: {analyses: [], options: ["cancel"]}}}
        verify(!acknowledgement.checked)
        verify(!unchecked.enabled)
    }
    function test_known_policy_needs_separate_acknowledgement() {
        const dialog = createTemporaryObject(dialogComponent, testCase)
        dialog.check = {evidence: {issues: [{location: "analysis", reason: "unreadable"}], conflict: {analyses: [{result: "known"}], options: ["cancel", "preserve_copy", "break_evidence", "remove_analysis"]}}}
        dialog.open()
        tryCompare(dialog, "opened", true)
        verify(!findChild(dialog, "saveUnchecked").visible)
        verify(!findChild(dialog, "preserveInput").enabled)
        verify(!findChild(dialog, "breakEvidence").enabled)
        clickButton(findChild(dialog, "acknowledgeIncompleteEvidence"))
        verify(findChild(dialog, "preserveInput").enabled)
        verify(findChild(dialog, "breakEvidence").enabled)
    }
    Component { id: autoSaveComponent; AutoSaveEvidence {} }
    Component {
        id: saveController
        QtObject {
            signal saveQuestion()
            property int saves: 0
            property string owner: "original"
            property string message: "Save check changed"
            property bool incomplete: true
            function check_save(path) {
                return JSON.stringify({check: {session: owner, document: 0, token: "receipt", target: path,
                    evidence: {conflict: {analyses: [], options: []}, issues: incomplete ? [{location: "store", reason: "offline"}] : []}}})
            }
            function save_confirmed(path, token, policy, acknowledge) { saves++; incomplete = false; return false }
        }
    }
    function test_auto_save_failure_does_not_save_a_refreshed_scope() {
        const controller = createTemporaryObject(saveController, testCase)
        const item = createTemporaryObject(autoSaveComponent, testCase, {controller: controller, fileUrls: controller, evidenceService: controller})
        item.inspect(true)
        const dialog = findChild(item, "autoSaveEvidenceDialog")
        tryCompare(dialog, "opened", true)
        dialog.choice("cancel")
        compare(controller.saves, 1)
        tryCompare(dialog, "opened", false)
    }
    function test_auto_save_project_switch_cannot_redirect_retry() {
        const controller = createTemporaryObject(saveController, testCase)
        const item = createTemporaryObject(autoSaveComponent, testCase, {controller: controller, fileUrls: controller, evidenceService: controller})
        item.inspect(true)
        const dialog = findChild(item, "autoSaveEvidenceDialog")
        tryCompare(dialog, "opened", true)
        controller.owner = "different project"
        controller.incomplete = false
        item.inspect(true)
        compare(controller.saves, 0)
        tryCompare(dialog, "opened", false)
    }

}
