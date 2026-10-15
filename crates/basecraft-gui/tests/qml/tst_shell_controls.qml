import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 640; height: 300
    property int textSize: 14
    property bool dark: false
    AppTabButton {
        id: tab
        text: "Inactive document"
        font.pixelSize: host.textSize
        palette.window: host.dark ? "#202020" : "#faf2dc"
        palette.windowText: host.dark ? "#eeeeee" : "#302b22"
        palette.dark: "#303030"
    }
    AppRibbonButton { id: command; y: 90; text: "History"; font.pixelSize: host.textSize }
    AppIconButton { id: more; y: 160; iconName: "more" }
    FontMetrics { id: metrics; font: tab.font }
    TestCase {
        name: "ShellControls"
        when: windowShown
        function init() { failOnWarning(/Binding loop/) }
        function test_compact_and_readable_data() {
            const cases = []
            for (const dark of [false, true])
                for (const size of [14, 24])
                    for (const checked of [false, true])
                        cases.push({tag: `${dark}-${size}-${checked}`, dark, size, checked})
            return cases
        }
        function luminance(color) {
            function linear(v) { return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4) }
            return 0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
        }
        function test_compact_and_readable(data) {
            host.dark = data.dark; host.textSize = data.size; tab.checked = data.checked
            waitForPolish(host)
            const fg = luminance(tab.contentItem.color)
            const bg = luminance(tab.background.color)
            verify((Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05) >= 4.5)
            verify(tab.height >= metrics.height && tab.height <= metrics.height + 8)
            verify(command.height >= metrics.height && command.height <= Math.max(16, metrics.height) + 8, `height ${command.height}, font ${metrics.height}, content ${command.contentItem.implicitHeight}`)
            verify(more.height <= 26)
        }
    }
}
