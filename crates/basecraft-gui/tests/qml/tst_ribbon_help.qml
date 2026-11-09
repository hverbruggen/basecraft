import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: scene
    width: 420; height: 240
    property int shortcutsCount: 0
    property int aboutCount: 0
    property string ribbonTabName: "Tree"
    property bool dark: false
    AppCommand { id: shortcuts; commandId: "test.shortcuts"; text: "Keyboard shortcuts"; onTriggered: scene.shortcutsCount++ }
    AppCommand { id: about; commandId: "test.about"; text: "About Basecraft"; onTriggered: scene.aboutCount++ }
    AppCommand { id: methods; commandId: "test.methods"; text: "Methods"; property string tooltip: "Open a document to read its methods"; enabled: false }
    AppCommand { id: learn; commandId: "help.learn"; text: "Manual" }
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
                methodsAction: methods
                learnAction: learn
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
            const met = findChild(help, "ribbonMethods")
            const figure = findChild(help, "ribbonFigureExport")
            const gear = findChild(help, "ribbonHelpPreferences")
            const separator = findChild(help, "ribbonHelpSeparator")
            verify(keys && info && docs && met && figure && gear && separator)
            for (const dark of [false, true]) {
                scene.dark = dark
                for (const scale of [0.75, 1, 1.5, 2]) {
                    ribbon.ribbonSizeFactor = scale
                    waitForRendering(help)
                    compare(keys.display, AbstractButton.IconOnly)
                    compare(info.display, AbstractButton.IconOnly)
                    verify(info.icon.source.toString().endsWith("basecraft-monochrome.svg"))
                    compare(info.icon.color, info.palette.buttonText)
                    verify(info.icon.width > keys.icon.width)
                    verify(info.mapToItem(help, 0, 0).x > keys.mapToItem(help, 0, 0).x)
                    verify(docs.mapToItem(help, 0, 0).y > keys.mapToItem(help, 0, 0).y)
                    fuzzyCompare(info.mapToItem(help, 0, 0).y, keys.mapToItem(help, 0, 0).y, 1)
                    verify(met.mapToItem(help, 0, 0).x < keys.mapToItem(help, 0, 0).x)
                    fuzzyCompare(figure.mapToItem(help, 0, 0).x, met.mapToItem(help, 0, 0).x, 1)
                    fuzzyCompare(docs.mapToItem(help, 0, 0).x, keys.mapToItem(help, 0, 0).x, 1)
                    fuzzyCompare(gear.mapToItem(help, 0, 0).x, info.mapToItem(help, 0, 0).x, 1)
                    verify(separator.width > 0 && separator.height >= keys.height)
                    verify(gear.mapToItem(help, gear.width, gear.height).y <= help.height + 1)
                    compare(docs.enabled, true)
                    verify(met.visible)
                    verify(!met.enabled)
                    compare(met.ToolTip.text, "Open a document to read its methods")

                }
            }
        }
        function test_disabled_methods_reason() {
            const met = findChild(help, "ribbonMethods")
            verify(!met.enabled)
            mouseMove(met, met.width / 2, met.height / 2)
            tryCompare(met.parent.ToolTip, "visible", true)
            compare(met.parent.ToolTip.text, "Open a document to read its methods")
            mouseMove(editor, 5, 5)
        }
        function test_manual_is_reserved() {
            const book = findChild(help, "ribbonHelpDocumentation")
            scene.ribbonTabName = "Tree"
            compare(book.Accessible.name, "Manual")
            compare(book.ToolTip.text, "Manual pages are not written yet")
            mouseClick(book)
            compare(scene.ribbonTabName, "Tree")
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
