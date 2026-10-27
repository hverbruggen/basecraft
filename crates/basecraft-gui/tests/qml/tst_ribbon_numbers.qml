import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 1000; height: 400
    Ribbon {
        id: ribbon
        width: 600; height: implicitHeight
        RibbonPage {
            RibbonGroup {
                title: "Size"
                RibbonNumberField {
                    id: size; objectName: "size"; label: "Size"; from: 6; to: 40; value: 13
                    onEdited: value => size.value = value
                }
            }
            RibbonGroup {
                title: "Zoom"
                RibbonNumberField {
                    id: zoom; objectName: "zoom"; label: "%"; from: 0; to: 20000; value: 100; fractional: true
                    onEdited: value => zoom.value = value
                }
            }
        }
    }
    AppSpinBox { id: dialogSpin; from: 6; to: 40; value: 13 }
    TestCase {
        name: "RibbonNumbers"
        when: windowShown
        function init() {
            size.value = 13; zoom.value = 100
            failOnWarning(/Binding loop|TypeError|ReferenceError/)
        }
        function test_editing_and_geometry_data() {
            return [1, 1.25, 1.5, 2].map(factor => ({tag: String(factor), factor}))
        }
        function test_editing_and_geometry(data) {
            ribbon.ribbonSizeFactor = data.factor
            verify(waitForPolish(ribbon.Window.window))
            const box = findChild(size, "sizeBox")
            compare(box.up.indicator.x, box.down.indicator.x)
            compare(box.up.indicator.y, 0)
            compare(box.down.indicator.y, box.height / 2)
            verify(box.up.indicator.height >= 16 * data.factor, "stepper retains a useful hit area: " + box.height)
            if (data.factor === 1) verify(box.width < dialogSpin.width, "one stepper column is narrower")
            box.forceActiveFocus()
            keyClick(Qt.Key_Up)
            compare(size.value, 14)
            size.value = 40
            box.increase()
            compare(box.value, 40)
            size.value = 6
            box.decrease()
            compare(box.value, 6)
            const input = box.contentItem
            input.forceActiveFocus()
            input.selectAll()
            for (const c of "27") keyClick(c)
            keyClick(Qt.Key_Return)
            compare(size.value, 27)
            const percent = findChild(zoom, "zoomBox")
            const editor = percent.contentItem
            for (const text of ["0.000001", "12.5", "20000"]) {
                editor.forceActiveFocus()
                editor.selectAll()
                for (const c of text) keyClick(c)
                keyClick(Qt.Key_Return)
                compare(zoom.value, Number(text), "fractional editing keeps precision")
                verify(waitForPolish(ribbon.Window.window))
                verify(editor.width >= editor.contentWidth, "current percentage fits")
            }
            zoom.value = 12.5
            percent.forceActiveFocus()
            keyClick(Qt.Key_Up)
            compare(zoom.value, 13.5)
            keyClick(Qt.Key_Down)
            compare(zoom.value, 12.5)
            editor.text = "20001"
            verify(!editor.acceptableInput)
            editor.text = "0"
            verify(!editor.acceptableInput)
        }
    }
}
