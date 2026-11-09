import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 1000; height: 500
    property int runs: 0
    property string selected: "File"
    TextField { id: editor; y: 250; width: 250 }
    RibbonTabs {
        id: tabs
        width: 500
        names: ["File", "View", "Sequence", "Export"]
        selectedName: host.selected
        onSelected: name => host.selected = name
        onEnterRibbon: backward => ribbon.enter(backward)
        onEscapeRequested: editor.forceActiveFocus()
    }
    Ribbon {
        id: ribbon
        y: 40; width: 950
        height: implicitHeight
        onEscapeRequested: editor.forceActiveFocus()
        RibbonPage {
            RibbonGroup {
                id: commands
                title: "File"
                AppCommandButton {
                    id: save
                    commandId: "file.save"
                    text: "Save"
                    iconName: "save"
                    onInvoked: host.runs++
                }
                AppCommandButton {
                    id: unavailable
                    commandId: "file.save-as"
                    text: "Save as"
                    iconName: "save-as"
                    enabled: false
                    disabledReason: "Open a document before saving a copy"
                    onInvoked: host.runs++
                }
            }
            RibbonGroup {
                id: zoom
                title: "Zoom"
                RibbonNumberField { id: number; label: "%"; tip: "Zoom percentage"; value: 100; from: 1; to: 200 }
            }
        }
    }
    RibbonKeyHints {
        id: hints
        anchors.fill: parent
        hostWindow: host.Window.window
        tabBar: tabs
        ribbon: ribbon
    }
    TestCase {
        name: "RibbonNavigation"
        when: windowShown
        function init() {
            failOnWarning(/Binding loop|TypeError|ReferenceError/)
            hints.stop(false); host.runs = 0
            ribbon.width = 950; commands.presentation = 0
            editor.text = ""; editor.forceActiveFocus(); wait(10)
        }
        function test_alt_release_and_consumed_letters() {
            keyPress(Qt.Key_Alt)
            compare(hints.level,0)
            keyRelease(Qt.Key_Alt)
            compare(hints.level,1)
            keyClick(Qt.Key_F); tryCompare(hints,"level",2)
            keyClick(Qt.Key_S)
            compare(host.runs,1)
            compare(editor.text,"")
        }
        function test_alt_chord_and_altgr() {
            keyPress(Qt.Key_Alt); keyClick(Qt.Key_X,Qt.AltModifier); keyRelease(Qt.Key_Alt)
            compare(hints.level,0)
            keyClick(Qt.Key_A,Qt.ControlModifier | Qt.AltModifier)
            compare(hints.level,0)
            keyClick(Qt.Key_AltGr)
            compare(hints.level,0)
        }
        function test_f10_escape_disabled_explanation() {
            keyClick(Qt.Key_F10); compare(hints.level,1)
            keyClick(Qt.Key_F); tryCompare(hints,"level",2)
            for (const letter of unavailable.commandAction.keyHint) keyClick(letter)
            compare(host.runs,0)
            verify(hints.explanation.includes("Open a document"))
            keyClick(Qt.Key_Escape); compare(hints.level,1)
            keyClick(Qt.Key_Escape); compare(hints.level,0)
            verify(editor.activeFocus)
        }
        function test_browse_disabled_and_edit_arrows() {
            ribbon.browse(commands,1)
            compare(ribbon.browsingControl,unavailable)
            const browser = findChild(ribbon,"ribbonBrowser")
            verify(browser.activeFocus)
            verify(browser.Accessible.description.includes("Open a document"))
            keyClick(Qt.Key_Return); compare(host.runs,0)
            keyClick(Qt.Key_Tab); compare(ribbon.browsingGroup,zoom)
            keyClick(Qt.Key_Return)
            const input = host.Window.window.activeFocusItem
            verify(input !== browser)
            keyClick(Qt.Key_Left)
            compare(host.Window.window.activeFocusItem,input)
        }
        function test_collapsed_group_retains_command_identity() {
            commands.presentation = 2
            commands.open(); wait(10)
            verify(save.visible)
            compare(commands.entries()[0],save)
            ribbon.activate(save)
            compare(host.runs,1)
            commands.close()
        }
    }
}
