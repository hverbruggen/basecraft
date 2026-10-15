import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 480; height: 420
    TextField { id: editor }
    QtObject {
        id: document
        function accessible_page(page) {
            const n = Math.max(0, Math.min(page, 1))
            return JSON.stringify({page: n, pages: 2, text: "<b>Literal data</b> page " + n})
        }
    }
    TextRepresentationDialog { id: dialog }
    TestCase {
        name: "TextRepresentation"
        when: windowShown
        function test_pages_and_focus() {
            editor.forceActiveFocus()
            dialog.source = document
            dialog.open()
            tryCompare(dialog.initialFocusItem, "activeFocus", true)
            compare(dialog.initialFocusItem.textFormat, TextEdit.PlainText)
            compare(dialog.page, 0)
            dialog.loadPage(1)
            compare(dialog.page, 1)
            verify(dialog.snapshot.text.includes("page 1"))
            verify(dialog.height <= parent.height)
            keyClick(Qt.Key_Escape)
            tryCompare(editor, "activeFocus", true)
            compare(dialog.source, null)
        }
    }
}
