import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: scene
    width: 420; height: 240
    property int shortcutsCount: 0
    property int aboutCount: 0
    property bool dark: false
    AppCommand { id: shortcuts; commandId: "test.shortcuts"; text: "Keyboard shortcuts"; onTriggered: scene.shortcutsCount++ }
    AppCommand { id: about; commandId: "test.about"; text: "About Basecraft"; onTriggered: scene.aboutCount++ }
    Rectangle { anchors.fill: parent; color: scene.dark ? "#1c2630" : "#f3eee4" }
    Ribbon {
        id: ribbon
        width: parent.width
        height: implicitHeight
        palette.buttonText: scene.dark ? "#eeeeee" : "#202020"
        RibbonPage {
            RibbonHelpGroup {
                id: help
                shortcutsAction: shortcuts
                aboutAction: about
                backgroundColor: scene.dark ? "#1c2630" : "#f3eee4"
            }
        }
    }
    TextInput { id: editor; x: 10; y: 190; width: 180; height: 30 }
    TestCase {
        name: "RibbonHelp"
        when: windowShown
        function test_grid_and_theme_scaling() {
            const keys = findChild(help, "ribbonHelpShortcuts")
            const info = findChild(help, "ribbonHelpAbout")
            const docs = findChild(help, "ribbonHelpDocumentation")
            const reserved = findChild(help, "ribbonHelpReserved")
            const separator = findChild(help, "ribbonHelpSeparator")
            verify(keys && info && docs && reserved && separator)
            for (const dark of [false, true]) {
                scene.dark = dark
                for (const scale of [0.75, 1, 1.5, 2]) {
                    ribbon.ribbonSizeFactor = scale
                    waitForRendering(help)
                    compare(keys.display, AbstractButton.IconOnly)
                    compare(info.display, AbstractButton.IconOnly)
                    verify(info.icon.source.toString().endsWith("basecraft-monochrome.svg"))
                    compare(info.icon.color, info.palette.buttonText)
                    verify(info.mapToItem(help, 0, 0).x > keys.mapToItem(help, 0, 0).x)
                    verify(docs.mapToItem(help, 0, 0).y > keys.mapToItem(help, 0, 0).y)
                    fuzzyCompare(info.mapToItem(help, 0, 0).y, keys.mapToItem(help, 0, 0).y, 1)
                    verify(reserved.mapToItem(help, 0, 0).x > info.mapToItem(help, 0, 0).x)
                    verify(separator.width > 0 && separator.height >= keys.height)
                    verify(reserved.mapToItem(help, reserved.width, reserved.height).y <= help.height + 1)
                    compare(docs.enabled, false)
                    compare(reserved.activeFocusOnTab, false)
                    compare(reserved.children.length, 0)

                }
            }
        }
        function test_actions_and_focus() {
            const keys = findChild(help, "ribbonHelpShortcuts")
            const info = findChild(help, "ribbonHelpAbout")
            editor.forceActiveFocus()
            const previousShortcuts = scene.shortcutsCount
            mouseClick(keys)
            compare(scene.shortcutsCount, previousShortcuts + 1)
            verify(editor.activeFocus, "Mouse ribbon use should preserve editor focus")
            const previousAbout = scene.aboutCount
            info.forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(scene.aboutCount, previousAbout + 1)
            about.enabled = false
            compare(info.enabled, false)
            about.enabled = true
            compare(keys.Accessible.name, "Keyboard shortcuts for this tab")
            compare(info.Accessible.name, "About Basecraft")
        }
    }
}
