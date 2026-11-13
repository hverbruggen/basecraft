import QtQuick

// Keep command state separate from AbstractButton.action. Action's enabled/text
// propagation must not feed back into the properties used to derive the command.
AppRibbonButton {
    id: button
    required property string commandId
    property var customization: ({shortcuts: {}, ribbon_hidden: []})
    property string hintScope: ""
    property string defaultKeyHint: ""
    property alias contextVisible: command.contextVisible
    readonly property alias commandAction: command
    signal invoked()
    text: ""
    visible: command.visible
    onClicked: command.trigger()
    AppCommand {
        id: command
        commandId: button.commandId
        customization: button.customization
        hintScope: button.hintScope
        defaultKeyHint: button.defaultKeyHint
        text: button.text.length ? button.text : button.tip
        tooltip: button.tip
        disabledReason: button.inheritedDisabledReason || button.disabledReason
        iconName: button.iconName
        enabled: button.enabled && command.visible
        onTriggered: button.invoked()
    }
}
