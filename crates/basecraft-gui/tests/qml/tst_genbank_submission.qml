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
            tryCompare(findChild(dialog, "sequenceSearch"), "activeFocus", true)
            return dialog
        }

        function sequenceControl(dialog, name, row) {
            const table = findChild(dialog, "submissionSequences")
            table.forceLayout()
            table.positionViewAtIndex(row, ListView.Contain)
            tryVerify(() => findChild(table.contentItem, name) !== null)
            return findChild(table.contentItem, name)
        }

        function test_frames_are_shown_for_review_and_confirmed() {
            const dialog = open({kind: "coding", gene: "rbcL", product: "ribulose-1,5-bisphosphate carboxylase/oxygenase large subunit"})
            tryCompare(findChild(dialog, "sequenceSearch"), "activeFocus", true)
            compare(sequenceControl(dialog, "frameText_HV101_rbcL", 0).text, "Detected 2: confirm")
            compare(sequenceControl(dialog, "frameText_HV103_rbcL", 1).text, "No clean frame")
            compare(sequenceControl(dialog, "frame_HV101_rbcL", 0).currentText, "2")
            sequenceControl(dialog, "confirmFrame_HV101_rbcL", 0).clicked()
            compare(frames.count, 1)
            compare(Array.from(frames.signalArguments[0]), ["HV101_rbcL", 2])
            // The user overrides HV103's frame with the keyboard.
            const choice = sequenceControl(dialog, "frame_HV103_rbcL", 1)
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
            dialog.currentTab = 3
            wait(0)
            findChild(dialog.body, "issue_0").clicked()
            compare(dialog.selectedRow, 0)
            tryCompare(sequenceControl(dialog, "decision_HV101_rbcL", 0), "activeFocus", true)
            verify(findChild(dialog.body, "translation").text.indexOf("GVGFKAG") > 0)
            dialog.currentTab = 3
            wait(0)
            findChild(dialog.body, "issue_1").clicked()
            tryCompare(sequenceControl(dialog, "decision_HV103_rbcL", 1), "activeFocus", true)
            // An issue about no record selects nothing.
            verify(!findChild(dialog.body, "issue_2").enabled)
            dialog.close()
        }

        function test_decisions_and_existing_accessions() {
            const dialog = open({kind: "rrna", product: "18S ribosomal RNA"})
            compare(sequenceControl(dialog, "accession_REF_rbcL", 2).text, "AB123456.1")
            const decision = sequenceControl(dialog, "decision_HV103_rbcL", 1)
            verify(decision.checked)
            decision.toggle(); decision.toggled()
            compare(Array.from(decisions.signalArguments[0]), ["HV103_rbcL", "exclude", ""])
            const first = sequenceControl(dialog, "decision_HV101_rbcL", 0)
            first.forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(Array.from(decisions.signalArguments[1]), ["HV101_rbcL", "exclude", ""])
            // Accession evidence remains separate from the inclusion checkbox.
            verify(sequenceControl(dialog, "accession_REF_rbcL", 2).text.includes("AB123456.1"))
            dialog.close()
        }

        function test_its_components_are_one_batch_edit() {
            const dialog = open({kind: "its", components: ["ITS1"]})
            dialog.currentTab = 1
            wait(0)
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
            verify(sequenceControl(dialog, "selectionReason_REF_rbcL", 2).text.includes("AB123456.1"))
            verify(sequenceControl(dialog, "selectionReason_REF_rbcL", 2).text.includes("unverified"))
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

        function test_marker_switch_uses_separate_drafts_and_does_not_move_focus_on_refresh() {
            const dialog = open({kind: "rrna", product: "18S ribosomal RNA"})
            dialog.currentTab = 1
            wait(0)
            const marker = findChild(dialog, "markerKind")
            marker.forceActiveFocus()
            marker.currentIndex = 0; marker.activated(0)
            const edit = JSON.parse(edits.signalArguments[0][0])
            compare(edit.batch.marker, {kind: "coding", gene: "", product: ""})
            const document = root.document({kind: "coding", gene: "tufA", product: "elongation factor Tu"})
            document.marker_drafts = {rrna: {kind: "rrna", product: "18S ribosomal RNA"}}
            dialog.document = document
            tryCompare(marker, "activeFocus", true)
            marker.currentIndex = 1; marker.activated(1)
            compare(JSON.parse(edits.signalArguments[1][0]).batch.marker, document.marker_drafts.rrna)
            dialog.close()
        }

        function test_large_tables_virtualize_filter_and_scroll_without_changing_inclusion() {
            const dialog = open({kind: "rrna", product: "18S"})
            const rows = []
            for (let i = 0; i < 964; i++) rows.push(Object.assign({}, root.review.rows[0],
                {record: "sequence_" + i, row: i, decision: i % 2 ? "new" : "exclude"}))
            dialog.review = {rows: rows, issues: [], exported: 482, selection_hash: "hash"}
            const table = findChild(dialog, "submissionSequences")
            wait(0)
            compare(table.count, 964)
            verify(table.contentItem.children.length < 100, "Delegates must remain bounded by viewport, not row count")
            table.forceLayout()
            table.positionViewAtEnd()
            tryVerify(() => findChild(table.contentItem, "decision_sequence_963") !== null)
            dialog.selectRow(963)
            tryCompare(findChild(table.contentItem, "decision_sequence_963"), "activeFocus", true)
            const search = findChild(dialog, "sequenceSearch")
            search.text = "sequence_96"
            tryCompare(table, "count", 5)
            findChild(dialog, "includeAllRows").clicked()
            compare(JSON.parse(edits.signalArguments[0][0]).rows.length, 5)
            compare(rows[0].decision, "exclude", "Preview/filtering do not mutate source decisions")
            dialog.close()
        }

        function test_publication_toggle_preserves_custom_drafts_and_labels() {
            const dialog = open({kind: "rrna", product: "18S"})
            dialog.currentTab = 2
            wait(0)
            const differs = findChild(dialog, "publicationDiffers")
            verify(!differs.checked)
            const authors = findChild(dialog, "publicationAuthors")
            const title = findChild(dialog, "publicationTitle")
            differs.toggle(); differs.toggled()
            authors.text = "O'Neill-Smith, Jane M."
            title.text = "Custom paper"
            differs.toggle(); differs.toggled()
            verify(!authors.visible)
            differs.toggle(); differs.toggled()
            compare(authors.text, "O'Neill-Smith, Jane M.")
            compare(title.text, "Custom paper")
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
