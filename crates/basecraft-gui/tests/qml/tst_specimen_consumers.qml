import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 1000; height: 700
    property var specimens: Array.from({length: 500}, (_, i) => ({id: "specimen-" + i}))
    property var records: Array.from({length: 500}, (_, i) => ({id: "record-" + i, attributes: {}}))
    QtObject {
        id: metadata
        property bool available: true
        property string last: ""
        function consumer(request, apply) {
            const r = JSON.parse(request)
            last = r.action
            if (r.action === "labels") return JSON.stringify({labels: r.records.map(id => ({record: id, specimen: null, issue: "Unlinked"}))})
            return JSON.stringify({operation: {params: {edit: {links: r.links}}}})
        }
    }
    QtObject {
        id: target
        property bool dirty: false
        property string workspace_json: JSON.stringify({specimens: host.records, hypotheses: [{id: "result", name: "Result"}]})
        property string last: ""
        function record_revision() { return "revision-1" }
        function style_json() { return JSON.stringify({style: {}, tips: host.records.map(s => s.id)}) }
        function compose_metadata_labels(project, fields) { last = fields; return "" }
        function action(name, args) { last = name }
    }
    Component { id: rosterFactory; SpecimenRosterDialog { metadataController: metadata; table: ({rows: host.specimens, partitions: []}) } }
    Component { id: labelsFactory; SpecimenTreeLabelsDialog { metadataController: metadata; table: ({rows: host.specimens, partitions: []}) } }
    TestCase {
        name: "SpecimenConsumers"
        when: windowShown
        function init() { failOnWarning(/Binding loop|TypeError|ReferenceError/) }
        function test_large_lists_data() {
            return [{tag: "roster-regular", roster: true, width: 1000, height: 700, font: 14},
                    {tag: "tree-regular", roster: false, width: 1000, height: 700, font: 14},
                    {tag: "roster-compact", roster: true, width: 800, height: 600, font: 18},
                    {tag: "tree-compact", roster: false, width: 800, height: 600, font: 18}]
        }
        function reachable(item) {
            verify(item.visible && item.width > 0 && item.height > 0)
            const p = item.mapToItem(host, 0, 0)
            verify(p.y >= 0 && p.y + item.height <= host.height + 1, "Control must remain within window: " + p.y)
            verify(p.x >= 0 && p.x + item.width <= host.width + 1)
        }
        function test_large_lists(data) {
            host.width = data.width; host.height = data.height
            host.Window.window.width = data.width; host.Window.window.height = data.height
            const dialog = createTemporaryObject(data.roster ? rosterFactory : labelsFactory, host)
            dialog.font.pixelSize = data.font
            dialog.show(target, data.roster ? "result.spart" : "tree.nwk")
            tryCompare(dialog, "opened", true)
            const list = findChild(dialog, data.roster ? "workspaceRosterLinks" : "treeRecordLinks")
            tryCompare(list.view, "count", 500)
            verify(waitForPolish(list.Window.window))
            verify(list.height <= 240 && list.height >= 45, "Bounded viewport: " + list.height)
            verify(list.view.contentHeight > list.view.height * 10)
            list.view.positionViewAtIndex(499, ListView.Contain)
            tryVerify(() => list.view.contentY > 0)
            tryVerify(() => list.view.itemAtIndex(499) !== null)
            const choice = findChild(list.view.itemAtIndex(499), "specimenLinkChoice")
            choice.forceActiveFocus()
            tryVerify(() => choice.activeFocus)
            reachable(choice)
            reachable(dialog.standardButton(Dialog.Close))
            if (data.roster) {
                reachable(findChild(dialog, "applyRosterLinks"))
                reachable(findChild(dialog, "saveProjectPartition"))
                const partitionId = findChild(dialog, "projectPartitionId")
                partitionId.text = "saved-result"
                mouseClick(findChild(dialog, "applyRosterLinks"))
                compare(target.last, "roster_links")
            } else {
                reachable(findChild(dialog, "saveTreeRecordLinks"))
                reachable(dialog.standardButton(Dialog.Apply))
                reachable(dialog.standardButton(Dialog.Reset))
                list.edited("record-499", "specimen-499")
                mouseClick(dialog.standardButton(Dialog.Apply))
                compare(metadata.last, "link_records")
                compare(target.last, '["genus","species","specimen"]')
            }
            dialog.close()
        }
    }
}
