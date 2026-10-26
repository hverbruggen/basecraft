import QtQuick
import QtQuick.Controls
import QtTest
import "../../qml"

Item {
    width: 1000; height: 760
    QtObject {
        id: fake
        property bool busy: false
        property var calls: []
        property string copied: ""
        property int changes: 0
        property string review_settings: JSON.stringify({strong_score:30,paired_score:20,ambiguity_percent:35,allow_weak_disagreement:true,weak_support_percent:80,check_reference:true,reference_percent:0,reference_min_rows:1})
        function add_sequences(text, name, mafft, orientation, keep) { calls.push({text,name,mafft,orientation,keep}) }
        function add_reviewed_sequences(text, name, mafft, orientation, keep, evidence) { calls.push({text,name,mafft,orientation,keep,evidence}) }
        function apply_review_settings(text) { changes++; review_settings = text; return true }
    }
    AddSequenceDialog { id: plain; controller: fake }
    AddSequenceDialog { id: fasta; controller: fake; inputKind: "fasta" }
    AddSequenceDialog { id: reviewed; controller: fake; inputKind: "reviewed" }
    AdditionErrorDialog { id: error; onCopyMessage: text => fake.copied = text }
    TextField { id: invoker; text: "Alignment" }
    AdditionGapsDialog { id: gaps; count: 2; additionDialog: plain }
    SignalSpy { id: gapRejected; target: gaps; signalName: "rejected" }
    SignalSpy { id: additionRejected; target: plain; signalName: "rejected" }
    TraceReviewControls { id: controls; controller: fake; width: 1000 }
    Component {
        id: largeTextControls
        Control {
            font.pixelSize: 24
            implicitWidth: thresholdControls.implicitWidth
            implicitHeight: thresholdControls.implicitHeight
            TraceReviewControls { id: thresholdControls; controller: fake }
        }
    }
    SignalSpy { id: inputErrors; target: plain; signalName: "inputError" }
    TestCase {
        name: "AdditionControls"
        when: windowShown
        function cleanup() { gaps.close(); plain.close(); fasta.close(); reviewed.close(); error.close(); fake.calls = []; inputErrors.clear() }
        function test_gap_cancellation_data() { return [{tag: "escape"}, {tag: "abort-button"}] }
        function test_gap_cancellation(data) {
            invoker.forceActiveFocus()
            plain.prepare("ACGT", "pending_sample")
            tryCompare(findChild(plain.contentItem, "sequenceName"), "activeFocus", true)
            gapRejected.clear(); additionRejected.clear()
            gaps.open()
            const abort = findChild(gaps.contentItem, "abortAddition")
            tryCompare(abort, "activeFocus", true)
            keyClick(data.tag === "escape" ? Qt.Key_Escape : Qt.Key_Space)
            tryCompare(gaps, "opened", false)
            tryCompare(plain, "opened", false)
            compare(gapRejected.count, 1)
            compare(additionRejected.count, 1)
            compare(fake.calls.length, 0, "Cancelling never retries or starts an addition")
            tryCompare(invoker, "activeFocus", true)
        }
        function test_review_fields_inherit_enlarged_ui_font() {
            const wrapper = createTemporaryObject(largeTextControls, parent)
            verify(wrapper)
            const input = findChild(wrapper, "strongScoreInput")
            verify(input)
            compare(input.font.pixelSize, 24)
            compare(input.Accessible.labelledBy.font.pixelSize, 24)
            verify(input.height >= input.contentItem.implicitHeight)
            verify(input.width >= input.contentItem.implicitWidth + input.leftPadding + input.rightPadding)
        }
        function test_plain_and_fasta_have_distinct_entry_fields() {
            plain.prepare("ACGT", "sample", 0)
            tryCompare(plain, "opened", true)
            verify(findChild(plain.contentItem, "sequenceName").visible)
            compare(findChild(plain.contentItem, "sequenceName").Accessible.name, "Sequence name")
            compare(findChild(plain.contentItem, "sequenceInput").Accessible.labelledBy.text, "Sequence")
            compare(findChild(plain.contentItem, "additionMode").Accessible.name, "Addition method")
            compare(findChild(plain.contentItem, "sequenceInput").placeholderText, "ACGT…")
            plain.submit(false)
            compare(fake.calls[0].name, "sample")
            plain.close()
            fasta.prepare(">one\nAC\n>two\nGT", "", 0)
            tryCompare(fasta, "opened", true)
            verify(!findChild(fasta.contentItem, "sequenceName").visible)
            compare(findChild(fasta.contentItem, "sequenceInput").Accessible.name, "FASTA records (names come from their headers)")
            fasta.submit(false)
            compare(fake.calls[1].name, "")
            verify(fake.calls[1].text.startsWith(">one"))
        }
        function test_plain_dialog_rejects_fasta_without_losing_input() {
            plain.prepare(">one\nAC", "sample")
            plain.submit(true)
            compare(inputErrors.count, 1)
            compare(fake.calls.length, 0)
            compare(findChild(plain.contentItem, "sequenceInput").text, ">one\nAC")
        }
        function test_reviewed_consensus_stays_hidden_and_keeps_its_id() {
            const evidence = JSON.stringify({schema_version: 1, id: "checked-review", hash: "b3:record"})
            reviewed.prepare("ACGT", "reviewed_sample", 0, evidence)
            tryCompare(reviewed, "opened", true)
            verify(!findChild(reviewed.contentItem, "sequenceInput").visible)
            reviewed.submit(false)
            compare(fake.calls[0].name, "reviewed_sample")
            compare(fake.calls[0].text, "ACGT")
            compare(fake.calls[0].evidence, evidence, "The selected review descriptor reaches the typed addition adapter")
            compare(fake.calls[0].mafft, false)
        }
        function test_error_is_persistent_copyable_and_enter_closes_it() {
            const message = "These IDs are already in the alignment:\n\none\ntwo"
            plain.prepare("ACGT", "one", 0)
            tryCompare(plain, "opened", true)
            error.show(message)
            tryCompare(error, "opened", true)
            const errorText = findChild(error.contentItem, "additionErrorText")
            compare(errorText.Accessible.name, "Cannot add sequences")
            compare(errorText.Accessible.description, message)
            const close = findChild(error.contentItem, "additionErrorClose")
            tryVerify(() => close.activeFocus)
            mouseClick(findChild(error.contentItem, "additionErrorCopy"))
            compare(fake.copied, message)
            verify(error.opened)
            mouseClick(parent, 5, 700)
            verify(error.opened, "clicking outside must not dismiss the message")
            close.forceActiveFocus()
            keyClick(Qt.Key_Return)
            tryCompare(error, "opened", false)
            verify(plain.opened, "closing the error preserves the addition dialog")
        }
        function test_live_thresholds_apply_once_and_follow_undo() {
            fake.changes = 0
            const before = fake.review_settings
            const input = findChild(controls, "strongScoreInput")
            input.forceActiveFocus()
            keyClick(Qt.Key_Up)
            tryVerify(() => JSON.parse(fake.review_settings).strong_score === 31)
            compare(fake.changes, 1)
            fake.review_settings = before
            tryCompare(input, "value", 30)
            compare(fake.changes, 1, "restoring settings must not trigger a new operation")
        }
    }
}
