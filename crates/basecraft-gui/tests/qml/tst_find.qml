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
        function find_query(text, rc) { query = text; reverse = rc; current = 1 }
        function find_poll(direction) {
            current = direction === 0 ? current : (current + (direction > 0 ? 1 : 2) - 1) % 3 + 1
            const count = query.length && query !== "missing" ? 3 : 0
            return JSON.stringify({count: count, current: count ? current : 0, label: "Read 2", nucleotide: true, y: 120, rows: []})
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
            tryVerify(() => bar.result.count === 3)
            compare(document.selection,17)
            verify(bar.queryField.activeFocus)
            keyClick(Qt.Key_Return)
            compare(bar.result.current,2)
            keyClick(Qt.Key_Return,Qt.ShiftModifier)
            compare(bar.result.current,1)
            keyClick(Qt.Key_Return,Qt.ShiftModifier)
            compare(bar.result.current,3)
            keyClick(Qt.Key_A,Qt.ControlModifier); keyClick(Qt.Key_Backspace)
            tryCompare(document,"query","")
            compare(bar.result.count,0)
            keyClick(Qt.Key_Escape)
            compare(bar.opened,false)
            tryCompare(editor,"activeFocus",true)
            compare(document.selection,17)
        }
        function test_no_match_does_not_reveal() {
            bar.query = "missing"; bar.openFind()
            wait(300)
            compare(bar.result.count,0); compare(document.reveals,0)
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
