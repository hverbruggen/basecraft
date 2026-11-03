import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtTest
import "../../qml"

Item {
    id: root
    width: 360
    height: 320
    Component {
        id: contentComponent
        AppContentDialog {
            id: contentDialog
            property bool trailingContent: false
            title: "Long explanation"
            standardButtons: Dialog.Close
            body: ColumnLayout {
                Label {
                    objectName: "explanation"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    text: "A long actionable error with retained input and instructions for retrying. ".repeat(60)
                }
                TextField { objectName: "retainedInput"; Layout.fillWidth: true; text: "Unsaved input" }
                Item { visible: contentDialog.trailingContent; Layout.preferredHeight: 300 }
            }
        }
    }
    TestCase {
        name: "ContentDialog"
        when: windowShown
        function test_window_relative_width_limit() {
            failOnWarning(/Binding loop/)
            const dialog = createTemporaryObject(contentComponent, root)
            dialog.open()
            tryCompare(dialog, "opened", true)
            tryVerify(() => Math.abs(dialog.width - dialog.hostWindow.width * 0.9) < 1)
            findChild(dialog.body, "explanation").text = "Short explanation"
            tryVerify(() => dialog.width < dialog.hostWindow.width * 0.9)
            dialog.close()
        }
        function test_reveal_after_content_reflow() {
            const dialog = createTemporaryObject(contentComponent, root, {trailingContent: true})
            dialog.open()
            tryCompare(dialog, "opened", true)
            const input = findChild(dialog.body, "retainedInput")
            tryCompare(input, "activeFocus", true)
            function revealed() {
                const p = input.mapToItem(dialog.contentItem, 0, 0)
                return p.y >= -1 && p.y + input.height <= dialog.contentItem.height + 1
            }
            tryVerify(revealed)
            // Layout can settle after focus has already scrolled the field.
            const before = dialog.contentItem.contentHeight
            findChild(dialog.body, "explanation").text += "More wrapped content. ".repeat(60)
            tryVerify(() => dialog.contentItem.contentHeight > before)
            tryVerify(revealed, 1000, "Reflow keeps the focused field visible without another focus event")
            verify(input.activeFocus)
        }
        function test_narrow_scrolling_data() { return [{tag: "normal", size: 12}, {tag: "large", size: 24}] }
        function test_narrow_scrolling(data) {
            failOnWarning(/Binding loop|only supports Flickable/)
            const dialog = createTemporaryObject(contentComponent, root)
            dialog.font.pixelSize = data.size
            dialog.open()
            tryCompare(dialog, "opened", true)
            verify(dialog.width <= root.width)
            verify(dialog.height <= root.height)
            const scroll = dialog.contentItem
            tryVerify(() => scroll.contentHeight > scroll.height)
            compare(dialog.body.width, scroll.availableWidth)
            const input = findChild(dialog.body, "retainedInput")
            verify(input)
            compare(input.text, "Unsaved input")
            scroll.contentItem.contentY = scroll.contentHeight - scroll.height
            tryVerify(() => input.mapToItem(scroll, 0, 0).y < scroll.height)
            dialog.close()
        }
    }
}
