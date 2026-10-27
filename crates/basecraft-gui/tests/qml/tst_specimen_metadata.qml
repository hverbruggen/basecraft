import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: root
    width: 900; height: 700
    Component { id: tableFactory; SpecimenTableDialog {} }
    Component { id: importFactory; MetadataImportDialog {} }
    Component { id: reviewFactory; MetadataReviewDialog {} }
    SignalSpy { id: edited; signalName: "cellEdited" }
    SignalSpy { id: identified; signalName: "taxonEdited" }
    SignalSpy { id: added; signalName: "specimenAdded" }
    SignalSpy { id: planned; signalName: "planRequested" }
    SignalSpy { id: applied; signalName: "applyRequested" }
    SignalSpy { id: dropped; signalName: "dropped" }
    readonly property var table: ({
        available: true, notices: ["specimens.tsv was changed outside Basecraft since the last save"],
        columns: [{name: "eventDate", type: "date", description: "Collection date"}],
        rows: [{id: "HV1", cells: {eventDate: "2019-05"}, taxon: "Codium fragile", identifications: 1, links: 1},
               {id: "HV2", cells: {}, taxon: "", identifications: 0, links: 0}],
        addable: [{name: "country", description: "Country"}], taxa: 1, links: 1, patterns: []
    })

    TestCase {
        name: "SpecimenMetadata"
        when: windowShown
        function init() { failOnWarning(/Binding loop|TypeError|ReferenceError/) }

        function test_empty_project_offers_creation() {
            const dialog = createTemporaryObject(tableFactory, root)
            dialog.open()
            tryCompare(dialog, "opened", true)
            const create = findChild(dialog, "createSpecimenTable")
            verify(create.visible)
            tryCompare(create, "activeFocus", true)
            dialog.close()
        }

        function test_table_edits_are_reported_per_cell() {
            const dialog = createTemporaryObject(tableFactory, root, {table: root.table})
            edited.target = dialog; edited.clear()
            identified.target = dialog; identified.clear()
            added.target = dialog; added.clear()
            dialog.open()
            tryCompare(dialog, "opened", true)
            tryCompare(findChild(dialog, "newSpecimenId"), "activeFocus", true)
            keyClick("H"); keyClick("V"); keyClick("3"); keyClick(Qt.Key_Return)
            compare(added.count, 1)
            compare(added.signalArguments[0][0], "HV3")
            const cell = findChild(dialog, "cell_HV2_eventDate")
            compare(cell.Accessible.name, "eventDate of HV2")
            cell.forceActiveFocus()
            keyClick("2"); keyClick("0"); keyClick("2"); keyClick("0")
            keyClick(Qt.Key_Tab)
            compare(edited.count, 1)
            compare(edited.signalArguments[0], ["HV2", "eventDate", "2020"])
            // Leaving an unchanged cell edits nothing.
            const same = findChild(dialog, "cell_HV1_eventDate")
            same.forceActiveFocus()
            keyClick(Qt.Key_Tab)
            compare(edited.count, 1)
            const taxon = findChild(dialog, "taxon_HV2")
            taxon.forceActiveFocus()
            for (const c of "Ulva") keyClick(c)
            keyClick(Qt.Key_Tab)
            compare(identified.signalArguments[0], ["HV2", "Ulva"])
            dialog.close()
        }

        function test_header_roles_need_a_specimen_field() {
            const dialog = createTemporaryObject(importFactory, root)
            planned.target = dialog; planned.clear()
            dialog.show("headers", "/p/rbcL.fasta",
                {records: 2, fields: 3, rows: [{record: "HV1|C_t|2019", header: "HV1|C_t|2019", fields: ["HV1", "C_t", "2019"]}]},
                ["eventDate", "country"], [])
            tryCompare(dialog, "opened", true)
            compare(dialog.roles["0"], "specimen")
            dialog.setRole("0", "ignore")
            verify(!findChild(dialog, "planImport").enabled)
            dialog.setRole("0", "specimen")
            dialog.setRole("1", "taxon")
            dialog.setRole("2", "column:eventDate")
            const plan = findChild(dialog, "planImport")
            verify(plan.enabled)
            mouseClick(plan)
            compare(planned.count, 1)
            compare(JSON.parse(planned.signalArguments[0][1]), ["specimen", "taxon", "column:eventDate"])
            compare(planned.signalArguments[0][2], true)
            compare(planned.signalArguments[0][3], false)
            dialog.close()
        }

        function test_sheet_mapping_starts_from_preview() {
            const dialog = createTemporaryObject(importFactory, root)
            planned.target = dialog; planned.clear()
            dialog.show("sheet", "/p/specimens.ods",
                {headers: ["specimen_id", "Country", "notes"], rows: [], count: 4,
                 mapping: {specimen_id: "specimen", Country: "column:country", notes: "ignore"}},
                ["country"], [])
            tryCompare(dialog, "opened", true)
            compare(findChild(dialog, "role_Country").currentText, "country")
            findChild(dialog, "planImport").clicked()
            compare(JSON.parse(planned.signalArguments[0][1]).Country, "column:country")
            dialog.close()
        }

        function test_review_lists_issues_and_applies_once() {
            const dialog = createTemporaryObject(reviewFactory, root)
            applied.target = dialog; applied.clear()
            dropped.target = dialog; dropped.clear()
            dialog.show({summary: "2 new specimens, 3 values", empty: false, rows: 3, matched: 0,
                new_specimens: ["HV1", "HV2"], columns: ["eventDate"],
                issues: [{kind: "duplicate", row: 3, message: "specimen HV1 appears again", included: false}]}, "pattern")
            tryCompare(dialog, "opened", true)
            compare(findChild(dialog, "reviewSummary").text, "Will add or change: 2 new specimens, 3 values.")
            tryCompare(findChild(dialog, "rememberName"), "activeFocus", true)
            for (const c of "id|taxon") keyClick(c)
            dialog.standardButton(Dialog.Apply).clicked()
            compare(applied.count, 1)
            compare(applied.signalArguments[0][0], "id|taxon")
            tryCompare(dialog, "opened", false)
            dialog.show({summary: "no changes", empty: true, rows: 1, matched: 1, issues: []}, "mapping")
            tryCompare(dialog, "opened", true)
            verify(!dialog.standardButton(Dialog.Apply).enabled)
            dialog.reject()
            compare(dropped.count, 1)
        }
    }
}
