import QtQuick
import QtQuick.Controls.Basic
import QtTest
import QtQuick.Window
import "../../qml"

Item {
    id: root
    width: 1000; height: 760
    property string copied: ""
    QtObject {
        id: mock
        property string revision: "one"
        property string name: "alignment.fasta"
        function record_revision() { return name + revision }
        readonly property string history_revision: name + revision
        function document_record() { return JSON.stringify({ document: name, revision: revision, unsaved: true, methods: "Sequences were aligned.", warnings: [], history: [
            {id:"a",time:"",summary:"Aligned sequences",status:"Contributing",contributing:true,details:"MAFFT settings"},
            {id:"b",time:"",summary:"Deleted columns",status:"Undone / not contributing",contributing:false,details:"Undone"}
        ], paragraphs: [{activity_ids:["a"],text:"Sequences were aligned.",parameters:"settings"}], raw:"{}", history_text:"Aligned sequences"}) }
    }
    Component { id: component; DocumentRecordDialog { controller: mock; onCopyText: text => root.copied = text } }
    TestCase {
        name: "DocumentRecord"
        when: windowShown
        function init() { mock.name = "alignment.fasta"; mock.revision = "one"; root.width = 1000; root.height = 760; root.Window.window.width = 1000; root.Window.window.height = 760 }
        function test_current_methods_and_evidence() {
            failOnWarning(/Binding loop|ReferenceError|TypeError/)
            const dialog = createTemporaryObject(component, root)
            dialog.showPage(1)
            tryCompare(dialog, "opened", true)
            compare(dialog.report.methods, "Sequences were aligned.")
            findChild(dialog, "copyRecordText").clicked()
            compare(root.copied, "Sequences were aligned.")
            compare(dialog.historyRows.length, 1)
            const provenance = findChild(dialog, "openMethodsProvenance")
            verify(provenance.visible)
            provenance.clicked()
            compare(dialog.page, 0)
            dialog.showPage(1)
            dialog.showEvidence("a")
            compare(dialog.page, 0)
            compare(dialog.selectedId, "a")
            compare(dialog.historyRows.length, 2)
            dialog.close()
        }
        function test_selection_survives_refresh_and_clears_on_document_switch() {
            const dialog = createTemporaryObject(component, root)
            dialog.showPage(0)
            dialog.selectedId = "a"
            mock.revision = "two"
            dialog.refreshReport()
            compare(dialog.selectedId, "a")
            compare(dialog.report.revision, "two")
            mock.name = "tree.nwk"
            dialog.refreshReport()
            compare(dialog.selectedId, "")
            compare(dialog.report.document, "tree.nwk")
            dialog.close()
        }
        function test_narrow_large_font() {
            root.width = 560; root.height = 520
            root.Window.window.width = 560; root.Window.window.height = 520
            wait(30)
            const dialog = createTemporaryObject(component, root)
            dialog.font.pixelSize = 18
            dialog.showPage(1)
            tryCompare(dialog, "opened", true)
            verify(dialog.width <= root.width)
            verify(dialog.height <= root.height)
            const tabs = findChild(dialog, "recordTabs")
            verify(tabs !== null)
            compare(tabs.count, 3)
            verify(tabs.width <= dialog.availableWidth + 1)
            for (let i = 0; i < tabs.count; ++i) {
                const tab = tabs.itemAt(i)
                verify(tab.x + tab.width <= tabs.width + 1)
            }
            dialog.close()
        }
    }
}
