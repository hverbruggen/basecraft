import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

TestCase {
    id: testCase
    name: "AnalysisEvidenceDialog"
    width: 680
    height: 700
    when: windowShown
    Component { id: dialogComponent; AnalysisEvidenceDialog {} }
    SignalSpy { id: choiceSpy; signalName: "choice" }
    SignalSpy { id: rejectSpy; signalName: "rejected" }
    function test_choices_and_escape() {
        const dialog = createTemporaryObject(dialogComponent, testCase)
        dialog.conflict = {analyses: [{result: "genes.mafft.fasta"}, {result: "genes.nwk"}], options: ["cancel", "preserve_copy", "remove_analysis", "break_evidence"]}
        choiceSpy.target = dialog
        rejectSpy.target = dialog
        dialog.open()
        tryCompare(dialog, "opened", true)
        const preserve = findChild(dialog, "preserveInput")
        verify(preserve.enabled)
        mouseClick(preserve)
        compare(choiceSpy.signalArguments[0][0], "preserve_copy")
        mouseClick(findChild(dialog, "removeAnalysis"))
        compare(choiceSpy.signalArguments[1][0], "remove_analysis")
        mouseClick(findChild(dialog, "breakEvidence"))
        compare(choiceSpy.signalArguments[2][0], "break_evidence")
        dialog.conflict = {analyses: [{result: "missing.nwk"}], options: ["cancel", "remove_analysis", "break_evidence"]}
        verify(!preserve.enabled)
        dialog.forceActiveFocus()
        keyClick(Qt.Key_Escape)
        tryCompare(dialog, "visible", false)
        compare(rejectSpy.count, 1)
        compare(choiceSpy.count, 3)
    }
}
