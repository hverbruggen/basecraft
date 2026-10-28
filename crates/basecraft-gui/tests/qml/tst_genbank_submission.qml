import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: root
    width: 1000; height: 800
    Component { id: submissionFactory; SubmissionDialog {} }
    Component { id: submitterFactory; SubmitterDialog {} }
    SignalSpy { id: edits; signalName: "editRequested" }
    SignalSpy { id: frames; signalName: "frameRequested" }
    SignalSpy { id: decisions; signalName: "decisionRequested" }
    SignalSpy { id: confirmed; signalName: "confirmDetectedRequested" }
    SignalSpy { id: saved; signalName: "saved" }
    readonly property var codes: [{id: 1, name: "1: Standard"}, {id: 5, name: "5: Invertebrate Mitochondrial"}, {id: 11, name: "11: Bacterial, Archaeal and Plant Plastid"}]
    function document(marker) {
        return {version: 1, source: {document: "rbcL.afa", hash: "b3:x"}, rows: [], frames: [],
            batch: {marker: marker, mol_type: "genomic_dna", genetic_code: 11, ends: "auto", organelle: "chloroplast"},
            export: {trim_terminal_unknown: true}, publication: {title: "", authors: []}, accessions: [],
            authors_text: ""}
    }
    readonly property var review: ({
        exported: 2, blocking: 2, selection_hash: "b3:review", selection_confirmed: false,
        rows: [
            {record: "HV101_rbcL", row: 0, decision: "new", accession: "", specimen: "HV101", organism: "Caulerpa taxifolia",
             research_name: "", length: 307, frame: {candidates: [2], detected: 2, chosen: null, effective: 2, translation: "GVGFKAG", internal_stops: []},
             blocking: 1, review: 1},
            {record: "HV103_rbcL", row: 1, decision: "new", accession: "", specimen: "HV103", organism: "Caulerpa taxifolia",
             research_name: "", length: 306, frame: {candidates: [], detected: null, chosen: null, effective: null, translation: "", internal_stops: []},
             blocking: 1, review: 0},
            {record: "REF_rbcL", row: 2, decision: "existing", accession: "AB123456.1", specimen: "", organism: "",
             selection_reason: "Explicit user decision", accession_candidates: [{accession: "AB123456.1", source: "submission accession"}],
             research_name: "", length: 0, frame: null, blocking: 0, review: 0}
        ],
        issues: [
            {severity: "blocking", source: "basecraft", code: "frame_unconfirmed", message: "Detected reading frame codon_start 2; confirm or change it", record: "HV101_rbcL", row: 0},
            {severity: "blocking", source: "basecraft", code: "no_clean_frame", message: "No reading frame is free of stop codons", record: "HV103_rbcL", row: 1},
            {severity: "blocking", source: "basecraft", code: "submitter", message: "Submitter email is not set", record: null, row: null}
        ]
    })

    TestCase {
        name: "GenbankSubmission"
        when: windowShown
        function init() { failOnWarning(/Binding loop|TypeError|ReferenceError/) }

        function open(marker) {
            const dialog = createTemporaryObject(submissionFactory, root, {
                document: root.document(marker), review: root.review, geneticCodes: root.codes,
                path: "submissions/rbcL.submission.json"})
            for (const spy of [edits, frames, decisions, confirmed]) { spy.target = dialog; spy.clear() }
            dialog.open()
            tryCompare(dialog, "opened", true)
            // Initial focus settles after opening; tests move it afterwards.
            tryCompare(findChild(dialog, "markerKind"), "activeFocus", true)
            return dialog
        }

        function test_frames_are_shown_for_review_and_confirmed() {
            const dialog = open({kind: "coding", gene: "rbcL", product: "ribulose-1,5-bisphosphate carboxylase/oxygenase large subunit"})
            tryCompare(findChild(dialog, "markerKind"), "activeFocus", true)
            compare(findChild(dialog.body, "frameText_HV101_rbcL").text, "Detected 2: confirm")
            compare(findChild(dialog.body, "frameText_HV103_rbcL").text, "No clean frame")
            compare(findChild(dialog.body, "frame_HV101_rbcL").currentText, "2")
            findChild(dialog.body, "confirmFrame_HV101_rbcL").clicked()
            compare(frames.count, 1)
            compare(Array.from(frames.signalArguments[0]), ["HV101_rbcL", 2])
            // The user overrides HV103's frame with the keyboard.
            const choice = findChild(dialog.body, "frame_HV103_rbcL")
            choice.forceActiveFocus()
            keyClick(Qt.Key_Down)
            compare(frames.count, 2)
            compare(Array.from(frames.signalArguments[1]), ["HV103_rbcL", 2])
            findChild(dialog.body, "confirmFrames").clicked()
            compare(confirmed.count, 1)
            compare(findChild(dialog.body, "geneticCode").currentText, "11: Bacterial, Archaeal and Plant Plastid")
            dialog.close()
        }

        function test_issue_selects_its_row_and_shows_translation() {
            const dialog = open({kind: "coding", gene: "rbcL", product: "x"})
            findChild(dialog.body, "issue_0").clicked()
            compare(dialog.selectedRow, 0)
            tryCompare(findChild(dialog.body, "decision_HV101_rbcL"), "activeFocus", true)
            verify(findChild(dialog.body, "translation").text.indexOf("GVGFKAG") > 0)
            findChild(dialog.body, "issue_1").clicked()
            tryCompare(findChild(dialog.body, "decision_HV103_rbcL"), "activeFocus", true)
            // An issue about no record selects nothing.
            verify(!findChild(dialog.body, "issue_2").enabled)
            dialog.close()
        }

        function test_decisions_and_existing_accessions() {
            const dialog = open({kind: "rrna", product: "18S ribosomal RNA"})
            compare(findChild(dialog.body, "accession_REF_rbcL").text, "AB123456.1")
            const decision = findChild(dialog.body, "decision_HV103_rbcL")
            decision.currentIndex = 2
            decision.activated(2)
            compare(Array.from(decisions.signalArguments[0]), ["HV103_rbcL", "exclude", ""])
            // Choosing an existing accession asks for it before saving.
            const first = findChild(dialog.body, "decision_HV101_rbcL")
            first.currentIndex = 1
            first.activated(1)
            compare(decisions.count, 1)
            const accession = findChild(dialog.body, "accession_HV101_rbcL")
            tryCompare(accession, "activeFocus", true)
            for (const c of "MZ123456") keyClick(c)
            keyClick(Qt.Key_Tab)
            compare(decisions.count, 2)
            compare(Array.from(decisions.signalArguments[1]), ["HV101_rbcL", "existing", "MZ123456"])
            dialog.close()
        }

        function test_its_components_are_one_batch_edit() {
            const dialog = open({kind: "its", components: ["ITS1"]})
            verify(findChild(dialog.body, "its_ITS1").checked)
            verify(!findChild(dialog.body, "markerProduct").visible)
            const box = findChild(dialog.body, "its_5.8S")
            box.toggle()
            box.toggled()
            compare(edits.count, 1)
            const edit = JSON.parse(edits.signalArguments[0][0])
            compare(edit.action, "set_batch")
            compare(edit.batch.marker, {kind: "its", components: ["ITS1", "5.8S"]})
            compare(edit.batch.organelle, "chloroplast")
            compare(edit.batch.genetic_code, 11)
            dialog.close()
        }

        function test_bulk_selection_and_confirmation_gate_export() {
            const dialog = open({kind: "rrna", product: "18S ribosomal RNA"})
            const exportButton = findChild(dialog.body, "exportPackage")
            const confirmButton = findChild(dialog.body, "confirmSelection")
            verify(!exportButton.enabled)
            verify(confirmButton.enabled)
            verify(findChild(dialog.body, "selectionReason_REF_rbcL").text.includes("AB123456.1"))
            verify(findChild(dialog.body, "selectionReason_REF_rbcL").text.includes("unverified"))
            // Bulk actions are typed edits, never implicit confirmation.
            findChild(dialog.body, "includeAllRows").forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(edits.count, 1)
            const included = JSON.parse(edits.signalArguments[0][0])
            compare(included.action, "set_rows")
            compare(included.rows.length, 3)
            verify(included.rows.every(r => r.decision === "new"))
            findChild(dialog.body, "excludeAllRows").clicked()
            const excluded = JSON.parse(edits.signalArguments[1][0])
            verify(excluded.rows.every(r => r.decision === "exclude"))
            confirmButton.forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(JSON.parse(edits.signalArguments[2][0]),
                {action: "confirm_selection", selection_hash: "b3:review"})
            verify(!exportButton.enabled)
            dialog.review = Object.assign({}, root.review, {selection_confirmed: true})
            verify(exportButton.enabled)
            verify(!confirmButton.enabled)
            dialog.review = Object.assign({}, root.review, {selection_hash: "b3:changed", selection_confirmed: false})
            verify(!exportButton.enabled)
            verify(confirmButton.enabled)
            dialog.review = {rows: [], issues: [], selection_hash: null, selection_confirmed: false}
            verify(!confirmButton.enabled)
            verify(!findChild(dialog.body, "includeAllRows").enabled)
            dialog.close()
        }

        function test_submitter_needs_required_fields() {
            const dialog = createTemporaryObject(submitterFactory, root)
            saved.target = dialog; saved.clear()
            dialog.show({first_name: "Jane", last_name: "Doe", email: "", institution: "University of Melbourne", city: "Melbourne", country: "Australia"})
            tryCompare(dialog, "opened", true)
            tryCompare(findChild(dialog, "submitterFirstName"), "activeFocus", true)
            verify(!dialog.standardButton(Dialog.Save).enabled)
            const email = findChild(dialog, "submitterEmail")
            email.forceActiveFocus()
            for (const c of "jane@example.org") keyClick(c)
            verify(dialog.standardButton(Dialog.Save).enabled)
            dialog.standardButton(Dialog.Save).clicked()
            compare(saved.count, 1)
            compare(saved.signalArguments[0][0].email, "jane@example.org")
            compare(saved.signalArguments[0][0].middle_initials, "")
        }
    }
}
