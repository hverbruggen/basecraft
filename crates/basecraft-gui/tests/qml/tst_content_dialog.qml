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
            title: "Long explanation"
            standardButtons: Dialog.Close
            body: ColumnLayout {
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    text: "A long actionable error with retained input and instructions for retrying. ".repeat(60)
                }
                TextField { objectName: "retainedInput"; Layout.fillWidth: true; text: "Unsaved input" }
            }
        }
    }
    TestCase {
        name: "ContentDialog"
        when: windowShown
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
