import QtQuick
import QtQuick.Controls
import QtTest
import "../../qml"

Item {
    width: 240; height: 120
    Rectangle { anchors.fill: parent; color: "white" }
    AppIcon { id: icon; name: "add"; color: "red"; x: 20; y: 20; size: 32 }
    AppIconButton { id: button; x: 80; y: 20; iconName: "close"; description: "Close tab" }
    SignalSpy { id: clicks; target: button; signalName: "clicked" }
    TestCase {
        name: "Icons"
        when: windowShown
        function coloredPixels(picture, red, blue) {
            let count = 0
            for (let y = 0; y < picture.height; ++y)
                for (let x = 0; x < picture.width; ++x)
                    if (picture.red(x, y) === red && picture.green(x, y) === 0 && picture.blue(x, y) === blue) count++
            return count
        }
        function test_tint_and_theme_change() {
            tryVerify(() => coloredPixels(grabImage(parent), 255, 0) > 20)
            // Grab the scene: QtTest's item crop uses logical coordinates on a
            // physical screenshot at fractional display scales.
            let picture = grabImage(parent)
            verify(coloredPixels(picture, 255, 0) > 20)
            compare(coloredPixels(picture, 0, 255), 0)
            icon.color = "blue"
            tryVerify(() => coloredPixels(grabImage(parent), 0, 255) > 20)
            picture = grabImage(parent)
            compare(coloredPixels(picture, 255, 0), 0)
            verify(coloredPixels(picture, 0, 255) > 20)
        }
        function test_button_accessibility_and_click() {
            compare(button.Accessible.name, "Close tab")
            mouseClick(button)
            compare(clicks.count, 1)
            button.enabled = false
            mouseClick(button)
            compare(clicks.count, 1)
        }
        function test_new_ribbon_icons_render() {
            icon.color = "red"
            for (const name of ["provenance", "text-representation", "annotation-add", "annotation-edit", "annotation-delete", "tree", "tree-flip", "tree-small-on-top", "tree-large-on-top", "matrix", "align", "trim", "waveform"]) {
                icon.name = name
                tryVerify(() => coloredPixels(grabImage(parent), 255, 0) > 20, 1000, name + " did not render")
            }
        }
    }
}
