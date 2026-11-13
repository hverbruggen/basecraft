import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 1000; height: 700
    property int recordCount: 500
    property var specimens: Array.from({length: Math.max(4, recordCount)}, (_, i) => ({id: "specimen-" + i}))
    property var records: Array.from({length: recordCount}, (_, i) => ({id: "record-" + i, attributes: {}}))
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
        function compose_metadata_labels(fields) { last = fields; return "" }
        function action(name, args) { last = name }
    }
    Component {
        id: windowFactory
        ApplicationWindow {
            id: appWindow
            visible: true
            width: 1000; height: 700
            font.family: "monospace"
            font.pointSize: 9
            property alias roster: roster
            property alias labels: labels
            SpecimenRosterDialog { id: roster; metadataController: metadata; table: ({rows: host.specimens, partitions: []}) }
            SpecimenTreeLabelsDialog { id: labels; metadataController: metadata; table: ({rows: host.specimens, partitions: []}) }
        }
    }
    TestCase {
        name: "SpecimenConsumers"
        when: windowShown
        function init() { failOnWarning(/Binding loop|TypeError|ReferenceError/) }
        function test_large_lists_data() {
            const cases = []
            for (const roster of [true, false]) {
                const kind = roster ? "roster" : "tree"
                cases.push({tag: kind + "-four-native", roster, count: 4, width: 2340, height: 1302, points: 9})
                cases.push({tag: kind + "-500", roster, count: 500, width: 1000, height: 700, points: 9})
                cases.push({tag: kind + "-500-compact", roster, count: 500, width: 800, height: 600, points: 13})
            }
            cases.push({tag: "tree-500-no-edits-backtab", roster: false, count: 500, width: 1000, height: 700, points: 9, edit: false, backtab: true})
            return cases
        }
        function visibleInside(item, container) {
            const p = item.mapToItem(container, 0, 0)
            return item.visible && item.width > 0 && item.height > 0
                && p.y >= -1 && p.y + item.height <= container.height + 1
                && p.x >= -1 && p.x + item.width <= container.width + 1
        }
        function checkRow(window, list, row) {
            tryVerify(() => window.activeFocusItem && window.activeFocusItem.Accessible.name === "Project specimen for record-" + row,
                1000, "User keys reach logical row " + row + "; got " + (window.activeFocusItem ? window.activeFocusItem.Accessible.name + "/" + window.activeFocusItem.objectName + "/" + window.activeFocusItem : "none"))
            tryVerify(() => visibleInside(window.activeFocusItem, list.view), 1000,
                "Focused row " + row + " is fully inside its viewport")
        }
        function test_mouse_and_keyboard_alternate_data() {
            return [{tag: "roster", roster: true}, {tag: "tree", roster: false}]
        }
        function test_mouse_and_keyboard_alternate(data) {
            host.recordCount = 4
            const window = createTemporaryObject(windowFactory, host)
            window.requestActivate()
            tryCompare(window, "active", true)
            const dialog = data.roster ? window.roster : window.labels
            dialog.show(target, data.roster ? "result.spart" : "tree.nwk")
            tryCompare(dialog, "opened", true)
            // opened can precede the first scene frame: hit testing still uses
            // the old popup/delegate positions until rendering and layout settle.
            verify(waitForRendering(dialog.contentItem))
            verify(waitForPolish(window))
            const list = findChild(dialog, data.roster ? "workspaceRosterLinks" : "treeRecordLinks")
            tryVerify(() => dialog.width > 450 && list.width >= Math.floor(dialog.availableWidth) - 1)
            // Mix mouse entry on non-current rows with logical keyboard traversal.
            // Popup Home/Down/Enter must commit to the clicked row, not a delegate.
            for (const row of [0, 1, 3, 0, 2]) {
                const chooser = findChild(list.view.itemAtIndex(row), "specimenLinkChoice")
                verify(visibleInside(chooser, list.view))
                mouseClick(chooser)
                tryCompare(chooser.popup, "visible", true)
                keyClick(Qt.Key_Home)
                for (let down = 0; down <= row % 3; down++) keyClick(Qt.Key_Down)
                keyClick(Qt.Key_Return)
                tryCompare(chooser.popup, "visible", false)
                const assigned = data.roster ? dialog.links : dialog.pendingLinks
                compare(assigned["record-" + row], "specimen-" + (row % 3), "Mouse selection commits to the clicked row")
                checkRow(window, list, row)
                if (row < 3) {
                    keyClick(Qt.Key_Tab)
                    checkRow(window, list, row + 1)
                    keyClick(Qt.Key_Tab, Qt.ShiftModifier)
                    checkRow(window, list, row)
                }
            }
            const assigned = data.roster ? dialog.links : dialog.pendingLinks
            for (let row = 0; row < 4; row++) compare(assigned["record-" + row], "specimen-" + (row % 3))
            dialog.close()
        }
        function test_large_lists(data) {
            host.recordCount = data.count
            target.last = ""
            const window = createTemporaryObject(windowFactory, host, {width: data.width, height: data.height})
            window.font.pointSize = data.points
            window.requestActivate()
            tryCompare(window, "active", true)
            const dialog = data.roster ? window.roster : window.labels
            dialog.show(target, data.roster ? "result.spart" : "tree.nwk")
            tryCompare(dialog, "opened", true)
            // opened can precede the first scene frame: hit testing still uses
            // the old popup/delegate positions until rendering and layout settle.
            verify(waitForRendering(dialog.contentItem))
            verify(waitForPolish(window))
            const list = findChild(dialog, data.roster ? "workspaceRosterLinks" : "treeRecordLinks")
            tryCompare(list.view, "count", data.count)
            // Popup layout is separate from ApplicationWindow.contentItem polish.
            // Wait for settled geometry; do not mutate item geometry in the test.
            tryVerify(() => dialog.width > 450 && list.width >= Math.floor(dialog.availableWidth) - 1)
            verify(list.height <= 240 && list.height >= 45)
            if (data.count > 4) {
                verify(list.view.contentHeight > list.view.height * 10)
                verify(list.view.contentItem.children.length < 80, "Delegates remain virtualized")
            }
            const first = findChild(list.view.itemAtIndex(0), "specimenLinkChoice")
            verify(visibleInside(first, list.view))
            // Enter using a user click, then traverse and edit every row with keys.
            mouseClick(first)
            keyClick(Qt.Key_Escape)
            for (let row = 0; row < data.count; row++) {
                checkRow(window, list, row)
                if (data.edit !== false) {
                    for (let down = 0; down <= row % 3; down++) keyClick(Qt.Key_Down)
                    const assigned = data.roster ? dialog.links : dialog.pendingLinks
                    compare(assigned["record-" + row], "specimen-" + (row % 3), "The edited mapping belongs to this row")
                }
                keyClick(Qt.Key_Tab)
                wait(0)
            }
            const action = findChild(dialog, data.roster ? "applyRosterLinks" : data.edit === false ? "metadataLabelGenus" : "saveTreeRecordLinks")
            tryCompare(action, "activeFocus", true)
            verify(visibleInside(action, dialog.contentItem))
            // Shift+Tab re-enters at the last logical row and crosses recycled
            // delegate boundaries in reverse, without imperative focus/scroll calls.
            if (data.backtab) keyClick(Qt.Key_Backtab); else keyClick(Qt.Key_Tab, Qt.ShiftModifier)
            for (let row = data.count - 1; row >= 0; row--) {
                checkRow(window, list, row)
                if (data.backtab) keyClick(Qt.Key_Backtab); else keyClick(Qt.Key_Tab, Qt.ShiftModifier)
                wait(0)
            }
            tryVerify(() => !list.containsItem(window.activeFocusItem), 1000,
                "Backtab at the first row leaves the logical list")
            // Shared footer Tab handling is independently owned by WF-0041.
            verify(visibleInside(dialog.standardButton(Dialog.Close), dialog.background))
            if (data.count > 4) verify(list.view.contentItem.children.length < 80)
            if (data.roster) {
                verify(visibleInside(findChild(dialog, "saveProjectPartition"), dialog.contentItem))
                mouseClick(action)
                compare(target.last, "roster_links")
            } else {
                verify(visibleInside(dialog.standardButton(Dialog.Apply), dialog.background))
                mouseClick(dialog.standardButton(Dialog.Apply))
                if (data.edit !== false) compare(metadata.last, "link_records")
                compare(target.last, '["genus","species","specimen"]')
            }
            dialog.close()
        }
    }
}
