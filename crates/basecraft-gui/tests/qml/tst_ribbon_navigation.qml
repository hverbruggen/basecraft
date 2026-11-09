import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 1000; height: 500
    property int runs: 0
    property string selected: "File"
    Shortcut { sequence: "Ctrl+K"; onActivated: host.runs += 100 }
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
                AppRibbonButton {
                    id: browseMenu
                    text: "Browse"
                    iconName: "open"
                    property string keyHint: "B"
                    onClicked: actionMenu.popup(browseMenu,0,height)
                    RibbonMenu {
                        id: actionMenu
                        onOpened: hints.menuOpened(actionMenu)
                        RibbonMenuItem { action: save.commandAction }
                        RibbonMenuItem { action: unavailable.commandAction }
                    }
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
        parent: host.Window.window.contentItem
        anchors.fill: parent
        hostWindow: host.Window.window
        tabBar: tabs
        ribbon: ribbon
    }
    AppDialog {
        id: dialog
        standardButtons: Dialog.Cancel
        contentItem: AppButton { id: apply; text: "Apply"; onClicked: { host.runs++; dialog.close() } }
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
            keyClick(Qt.Key_Control)
            keyClick(Qt.Key_K,Qt.ControlModifier)
            compare(host.runs,0)
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
        function test_menu_levels_and_command_identity() {
            ribbon.width = 80
            tryCompare(commands,"presentation",2)
            keyClick(Qt.Key_F10); keyClick(Qt.Key_F); tryCompare(hints,"level",2)
            keyClick(Qt.Key_G); keyClick(Qt.Key_1); compare(hints.level,3)
            keyClick(Qt.Key_B); tryCompare(hints,"level",4)
            keyClick(Qt.Key_Escape); compare(hints.level,3)
            keyClick(Qt.Key_B); tryCompare(hints,"level",4)
            keyClick(Qt.Key_S); compare(host.runs,1)
            compare(hints.level,0)
            compare(editor.text,"")
        }
        function test_dialog_mnemonics_and_modal_exclusion() {
            dialog.open(); wait(20)
            hints.registerDialog(dialog)
            keyPress(Qt.Key_Alt)
            verify(hints.mnemonicsVisible)
            compare(hints.level,0)
            keyClick(Qt.Key_A,Qt.AltModifier)
            keyRelease(Qt.Key_Alt)
            compare(host.runs,1)
            hints.unregisterDialog(dialog)
        }
        function test_tabbar_enters_group() {
            tabs.currentItem.forceActiveFocus()
            wait(10)
            keyClick(Qt.Key_Tab)
            compare(ribbon.browsingGroup,commands)
            keyClick(Qt.Key_Escape)
            verify(editor.activeFocus)
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
