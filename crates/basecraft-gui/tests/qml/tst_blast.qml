import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 620; height: 520
    TextField { id: invoker }
    QtObject {
        id: backend
        property bool busy: false
        property string message: ""
        property string outcome: "{}"
        property int submissions: 0
        property bool lastTruncation: false
        property bool oversized: false
        property string previewError: ""
        property string defaultAlphabet: "nucleotide"
        property int preparations: 0
        function blast_default_alphabet(source) { return defaultAlphabet }
        function blast_contact() { return "test@example.org" }
        function preview_blast(source, settings) {
            preparations++
            if (previewError) return JSON.stringify({error: previewError})
            return JSON.stringify({approval_hash: "frozen", records: [{id:"query_1",name:"record",source:{ranges:[[0,8]]},original_length:8,submitted_length:4,shortened:true}], original_residues:8, submitted_residues:4, limit:4, oversized:oversized})
        }
        function submit_blast(source, settings, approval, truncate) {
            submissions++; lastTruncation = truncate; busy = true; return true
        }
        function poll() {}
        function cancel() { busy = false }
    }
    BlastDialog { id: dialog; backend: backend }
    TestCase {
        name: "BlastDialog"
        when: windowShown
        property string source: "frozen query"
        property int browserCalls: 0
        property int systemBrowserCalls: 0
        function init() {
            backend.submissions = 0; backend.busy = false; backend.outcome = "{}"
            backend.oversized = false; backend.previewError = ""; backend.defaultAlphabet = "nucleotide"; backend.preparations = 0
            source = "frozen query"; browserCalls = 0; systemBrowserCalls = 0
            dialog.openBrowser = url => { browserCalls++; return "failed" }
            dialog.openSystemBrowser = url => { systemBrowserCalls++; return true }
            invoker.forceActiveFocus()
            dialog.show(mode => source, "whole", false)
            tryCompare(dialog, "opened", true)
        }
        function cleanup() { backend.busy = false; dialog.close(); tryCompare(dialog, "visible", false) }
        function test_source_changes_invalidate_and_explicit_truncation() {
            backend.oversized = true
            dialog.search()
            compare(backend.submissions, 0)
            source = "changed query"
            dialog.start(true)
            compare(backend.submissions, 0)
            verify(dialog.formError.includes("Source or settings changed"))
            dialog.search()
            compare(backend.submissions, 0)
            findChild(dialog, "blastSearch").clicked()
            compare(backend.submissions, 1); verify(backend.lastTruncation)
            verify(dialog.height <= parent.height)
        }
        function test_browser_failure_retains_link_and_reopens_without_submission() {
            dialog.search()
            backend.busy = false
            const url = "https://blast.ncbi.nlm.nih.gov/Blast.cgi?CMD=Get&RID=ABCD1234567&FORMAT_TYPE=HTML"
            backend.outcome = JSON.stringify({state:"partial",data:{status:"accepted",url:url,message:"Submitted"}})
            dialog.completed()
            compare(browserCalls, 1); compare(dialog.resultUrl, url)
            verify(dialog.resultMessage.includes("Browser could not open"))
            dialog.openAgain(); compare(browserCalls, 2); compare(backend.submissions, 1)
            dialog.resultUrl = "https://example.org/?RID=ABCD1234567"
            dialog.openAgain(); compare(browserCalls, 2)
        }
        function test_normal_search_prepares_and_submits_once() {
            compare(backend.preparations, 0); compare(backend.submissions, 0)
            compare(findChild(dialog, "blastProgram").currentIndex, 0)
            findChild(dialog, "blastSearch").clicked()
            compare(backend.preparations, 1); compare(backend.submissions, 1)
            verify(!backend.lastTruncation)
            dialog.search(); compare(backend.submissions, 1)
        }
        function test_invalid_queries_send_nothing() {
            backend.previewError = "Invalid nucleotide residue"
            dialog.search()
            compare(backend.submissions, 0)
            compare(dialog.formError, backend.previewError)
        }
        function test_program_choices_and_inferred_protein_default() {
            backend.defaultAlphabet = "protein"
            dialog.show(mode => source, "selection", false)
            const program = findChild(dialog, "blastProgram")
            compare(program.currentIndex, 2)
            compare(findChild(dialog, "blastSource").currentIndex, 1)
            const alphabets = ["nucleotide", "nucleotide", "protein", "protein"]
            const databases = ["nt", "nr", "nr", "nt"]
            for (let i = 0; i < 4; i++) {
                program.currentIndex = i
                compare(JSON.parse(dialog.settings()).alphabet, alphabets[i])
                compare(JSON.parse(dialog.settings()).database, databases[i])
            }
        }
        function test_unsupported_browser_describes_fallback_without_resubmission() {
            dialog.openBrowser = url => "unsupported"
            dialog.resultUrl = "https://blast.ncbi.nlm.nih.gov/Blast.cgi?CMD=Get&RID=ABCD1234567&FORMAT_TYPE=HTML"
            dialog.openAgain()
            compare(systemBrowserCalls, 1)
            verify(dialog.resultMessage.includes("window or tab"))
            compare(backend.submissions, 0)
        }
        function test_unknown_and_escape_send_nothing() {
            backend.outcome = JSON.stringify({state:"completed",data:{status:"unknown",message:"Submission status unknown"}})
            dialog.completed(); compare(browserCalls, 0)
            dialog.forceActiveFocus(); keyClick(Qt.Key_Escape)
            tryCompare(dialog, "visible", false)
            compare(backend.submissions, 0)
            tryCompare(invoker, "activeFocus", true)
        }
        function test_settings_change_and_alignment_source_choices() {
            backend.oversized = true
            dialog.search()
            findChild(dialog, "blastContact").text = "changed@example.org"
            dialog.start(true); compare(backend.submissions, 0)
            dialog.show(mode => source,"rows",true)
            compare(findChild(dialog, "blastSource").count,3)
            compare(dialog.sourceMode,"rows")
        }
    }
}
