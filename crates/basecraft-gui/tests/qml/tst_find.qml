import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 760; height: 300
    TextField { id: editor; text: "unchanged scientific selection" }
    QtObject {
        id: document
        property string query: ""
        property bool reverse: false
        property int current: 1
        property int selection: 17
        property int reveals: 0
        property string find_status: ""
        property int find_count: 0
        property int find_row: 0
        property real find_hit_y: 120
        property bool find_nucleotide: true
        property string find_rows: "[]"
        property int find_reveals: 0
        function find_query(text, rc) { query = text; reverse = rc; current = 1; find_count = 0; find_status = text.length ? "Finding…" : "" }
        // Like a controller: a step moves and reveals; the first step after a
        // query finishes its scan and reveals the first hit.
        function find_step(direction) {
            const scanning = find_status === "Finding…"
            current = direction === 0 ? current : (current + (direction > 0 ? 1 : 2) - 1) % 3 + 1
            find_count = query.length && query !== "missing" ? 3 : 0
            find_status = find_count ? current + " of 3 · Read 2" : "No matches"
            if (find_count && (direction !== 0 || scanning)) find_reveals++
        }
    }
    FindBar { id: bar; anchors { left: parent.left; right: parent.right; bottom: parent.bottom } controller: document; documentFocusItem: editor; onRevealed: document.reveals++ }
    TestCase {
        name: "DocumentFind"
        when: windowShown
        function init() { failOnWarning(/.*/); bar.closeFind(); bar.query = ""; document.reveals = 0 }
        function test_typing_navigation_clear_escape() {
            editor.forceActiveFocus()
            bar.openFind()
            tryCompare(bar.queryField, "activeFocus", true)
            keyClick(Qt.Key_A); keyClick(Qt.Key_C); keyClick(Qt.Key_G)
            tryCompare(document, "query", "acg")
            tryVerify(() => document.find_count === 3)
            compare(document.selection,17)
            verify(bar.queryField.activeFocus)
            keyClick(Qt.Key_Return)
            compare(document.current,2)
            keyClick(Qt.Key_Return,Qt.ShiftModifier)
            compare(document.current,1)
            keyClick(Qt.Key_Return,Qt.ShiftModifier)
            compare(document.current,3)
            keyClick(Qt.Key_A,Qt.ControlModifier); keyClick(Qt.Key_Backspace)
            tryCompare(document,"query","")
            compare(document.find_count,0)
            keyClick(Qt.Key_Escape)
            compare(bar.opened,false)
            tryCompare(editor,"activeFocus",true)
            compare(document.selection,17)
        }
        function test_no_match_does_not_reveal() {
            bar.query = "missing"; bar.openFind()
            wait(300)
            compare(document.find_count,0); compare(document.reveals,0)
            verify(bar.queryField.activeFocus)
        }
        function test_reopen_retains_query_and_document_isolation() {
            bar.query = "acg"; bar.openFind(); wait(300)
            bar.closeFind(); compare(document.query,"")
            bar.openFind(); tryCompare(document,"query","acg")
            verify(bar.queryField.activeFocus)
        }
    }
}
