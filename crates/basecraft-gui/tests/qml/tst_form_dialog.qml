import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import QtTest
import "../../qml"

Item {
    width: 480
    height: 420
    Component {
        id: formComponent
        AppFormDialog {
            property alias lastField: lastField
            title: "Analysis settings"
            standardButtons: Dialog.Close
            SpinBox { Kirigami.FormData.label: "Workers"; from: 1; to: 128; editable: true }
            ComboBox { Kirigami.FormData.label: "Model"; model: ["Default", "Alternative"] }
            Repeater {
                model: 12
                SpinBox {
                    required property int index
                    Kirigami.FormData.label: "Long parameter label " + index
                    from: 0; to: 100
                }
            }
            SpinBox { id: lastField; Kirigami.FormData.label: "Last parameter"; editable: true }
        }
    }
    TestCase {
        name: "FormDialog"
        when: windowShown
        function test_bounded_form_data() { return [{tag: "normal", size: 12}, {tag: "large", size: 24}] }
        function test_bounded_form(data) {
            failOnWarning(/Binding loop/)
            const dialog = createTemporaryObject(formComponent, parent)
            verify(dialog)
            dialog.font.pixelSize = data.size
            dialog.open()
            tryCompare(dialog, "opened", true)
            tryVerify(() => dialog.lastField.Accessible.labelledBy.font.pixelSize === data.size)
            verify(dialog.width <= parent.width)
            verify(dialog.height <= parent.height)
            tryVerify(() => dialog.contentItem.contentHeight > dialog.contentItem.height, 1000, "long forms scroll: " + dialog.contentItem.contentHeight + "/" + dialog.contentItem.height)
            dialog.lastField.forceActiveFocus(Qt.TabFocusReason)
            tryVerify(() => {
                const point = dialog.lastField.mapToItem(dialog.contentItem, 0, 0)
                return point.y >= -1 && point.y + dialog.lastField.height <= dialog.contentItem.height + 1
            }, 1000, "Keyboard focus reveals the whole field")
            dialog.close()
        }
    }
}
