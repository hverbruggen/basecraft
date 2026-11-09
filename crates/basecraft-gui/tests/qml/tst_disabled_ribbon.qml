import QtQuick
import QtQuick.Controls.Basic
import QtTest
import "../../qml"

Item {
    width: 600; height: 300
    property int invocations: 0
    RibbonGroup {
        id: group
        width: implicitWidth; height: 100
        enabled: false
        disabledReason: "Open an alignment to use this group"
        AppCommandButton {
            id: commandButton
            commandId: "test.disabled"
            text: "Align"
            tip: "Realign sequences"
            disabledReason: "Wait for the analysis to finish"
            onInvoked: invocations++
        }
    }
    TextInput { id: editor; y: 180; width: 200; height: 30 }
    TestCase {
        name: "DisabledRibbon"
        when: windowShown
        function test_hover_reason_keeps_disabled_action_unavailable() {
            mouseMove(commandButton, commandButton.width / 2, commandButton.height / 2)
            tryCompare(commandButton.ToolTip, "visible", true)
            compare(commandButton.ToolTip.text, "Open an alignment to use this group")
            compare(commandButton.Accessible.description, commandButton.ToolTip.text)
            compare(commandButton.commandAction.effectiveTooltip, commandButton.ToolTip.text)
            mouseClick(commandButton)
            commandButton.commandAction.trigger()
            compare(invocations, 0)
            editor.forceActiveFocus()
            commandButton.forceActiveFocus()
            verify(!commandButton.activeFocus, "Disabled controls cannot receive Qt keyboard focus")
            group.enabled = true
            commandButton.enabled = false
            compare(commandButton.commandAction.effectiveTooltip, "Wait for the analysis to finish")
            commandButton.enabled = true
            compare(commandButton.commandAction.effectiveTooltip, "Realign sequences")
            commandButton.forceActiveFocus()
            keyClick(Qt.Key_Space)
            compare(invocations, 1)
        }
    }
}
