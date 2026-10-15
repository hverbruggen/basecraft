import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    id: host
    width: 520
    height: 500
    property int invocations: 0
    QtObject {
        id: store
        property string snapshot: JSON.stringify({ui: {show_gc: true, sidebar_open: true, orf_min_codons: 100}, files: {provenance: "portable", recovery: true}, compute: {local_cpu_threads: 4}, customization: {shortcuts: {}, ribbon_hidden: []}})
        property string lastKey
        property var lastValue
        function update(key, value) { lastKey = key; lastValue = JSON.parse(value); return true }
        function reload() { return true }
    }
    AppCommand {
        id: command
        commandId: "test.run"
        text: "Run"
        defaultShortcut: "Ctrl+R"
        onTriggered: host.invocations++
    }
    TextField { id: editor; x: 220; width: 160; text: "Editor focus" }
    AppCommandButton {
        id: ribbonButton
        commandId: "test.ribbon"
        text: "Ribbon command"
        tip: "Run the ribbon command"
        onInvoked: host.invocations++
    }
    PreferencesDialog { id: preferences; settings: store; commands: [command] }
    TestCase {
        name: "Preferences"
        when: windowShown
        function cleanup() { preferences.close(); command.customization = ({shortcuts: {}, ribbon_hidden: []}) }
        function test_overrides_missing_ids_and_reset() {
            command.customization = ({shortcuts: {"test.run": "Ctrl+Alt+R", "missing.plugin": "Ctrl+Alt+J"}, ribbon_hidden: ["test.run", "missing.plugin"]})
            compare(command.shortcut, "Ctrl+Alt+R")
            compare(command.visible, false)
            command.customization = ({shortcuts: {}, ribbon_hidden: []})
            compare(command.shortcut, "Ctrl+R")
            compare(command.visible, true)
        }
        function test_bounded_preferences_and_targeted_updates() {
            preferences.open()
            tryCompare(preferences, "opened", true)
            verify(preferences.width <= host.width)
            verify(preferences.height <= host.height)
            preferences.change("localCpuThreads", 8)
            compare(store.lastKey, "localCpuThreads")
            compare(store.lastValue, 8)
            preferences.change("resetCustomization", null)
            compare(store.lastKey, "resetCustomization")
        }
        function test_font_size_keyboard_and_reset() {
            preferences.open()
            tryCompare(preferences, "opened", true)
            const size = findChild(preferences, "interfaceFontSize")
            compare(size.value, preferences.defaultFontSizePoints)
            size.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Up)
            compare(store.lastKey, "fontSizePoints")
            compare(store.lastValue, preferences.defaultFontSizePoints + 1)
            findChild(preferences, "resetInterfaceFontSize").forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            compare(store.lastValue, null)
        }
        function test_contextual_ribbon_command_dispatch_and_focus() {
            const before = host.invocations
            ribbonButton.customization = ({shortcuts: {"test.ribbon": "Ctrl+Alt+K"}, ribbon_hidden: []})
            compare(ribbonButton.commandAction.enabled, true)
            editor.forceActiveFocus(Qt.TabFocusReason)
            verify(editor.activeFocus)
            mouseClick(ribbonButton)
            verify(editor.activeFocus)
            compare(host.invocations, before + 1)
            keyClick(Qt.Key_K, Qt.ControlModifier | Qt.AltModifier)
            compare(host.invocations, before + 2)
            ribbonButton.enabled = false
            compare(ribbonButton.commandAction.enabled, false)
            keyClick(Qt.Key_K, Qt.ControlModifier | Qt.AltModifier)
            compare(host.invocations, before + 2)
            ribbonButton.enabled = true
            ribbonButton.customization = ({shortcuts: {"test.ribbon": "Ctrl+Alt+K"}, ribbon_hidden: ["test.ribbon"]})
            compare(ribbonButton.visible, false)
            compare(ribbonButton.commandAction.enabled, false)
            keyClick(Qt.Key_K, Qt.ControlModifier | Qt.AltModifier)
            compare(host.invocations, before + 2)
            ribbonButton.customization = ({shortcuts: {}, ribbon_hidden: []})
        }
        function test_shared_action_keyboard_dispatch() {
            const before = host.invocations
            keyClick(Qt.Key_R, Qt.ControlModifier)
            compare(host.invocations, before + 1)
            command.customization = ({shortcuts: {"test.run": "Ctrl+Alt+R"}, ribbon_hidden: []})
            keyClick(Qt.Key_R, Qt.ControlModifier)
            compare(host.invocations, before + 1)
            keyClick(Qt.Key_R, Qt.ControlModifier | Qt.AltModifier)
            compare(host.invocations, before + 2)
        }
    }
}
