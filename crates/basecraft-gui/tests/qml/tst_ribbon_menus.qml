import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 800
    height: 600
    property int activations: 0
    property real factor: 1
    RibbonMenu {
        id: menu
        sizeFactor: factor
        RibbonMenuItem {
            id: first
            sizeFactor: factor
            text: "SVG"
            icon.source: "../../icons/figure-export.svg"
            onTriggered: activations++
        }
        RibbonMenuItem {
            id: second
            sizeFactor: factor
            text: "A longer export caption"
            onTriggered: activations++
        }
    }
    TestCase {
        name: "RibbonMenus"
        when: windowShown
        function cleanup() { menu.close() }
        function test_content_size_and_keyboard_activation() {
            for (const scale of [0.75, 1, 1.5, 2]) {
                factor = scale
                menu.popup(10, 10)
                tryCompare(menu, "opened", true)
                waitForRendering(menu.contentItem)
                compare(first.icon.width, 16 * scale)
                compare(first.icon.height, 16 * scale)
                verify(menu.width >= second.implicitWidth + menu.leftPadding + menu.rightPadding - 1)
                // A short label must not retain Basic's 200px menu minimum.
                const original = second.text
                second.text = "PNG"
                tryVerify(() => menu.width < 200)
                first.forceActiveFocus()
                menu.currentIndex = 0
                const previous = activations
                keyClick(Qt.Key_Return)
                compare(activations, previous + 1)
                tryCompare(menu, "visible", false)
                second.text = original
            }
        }
    }
}
