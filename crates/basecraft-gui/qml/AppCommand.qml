import QtQuick
import org.kde.kirigami as Kirigami

Kirigami.Action {
    id: command
    required property string commandId
    property var customization: ({ shortcuts: {}, ribbon_hidden: [] })
    // From the command list; the window supplies both.
    property string hintScope: ""
    property string defaultKeyHint: ""
    readonly property string keyHint: customization.key_hints && customization.key_hints[commandId] !== undefined ? customization.key_hints[commandId] : defaultKeyHint
    property string defaultShortcut: ""
    shortcut: customization.shortcuts && customization.shortcuts[commandId] !== undefined
        ? customization.shortcuts[commandId] : defaultShortcut
    property bool contextVisible: true
    visible: contextVisible && (!customization.ribbon_hidden || customization.ribbon_hidden.indexOf(commandId) < 0)
    property string disabledReason
    readonly property string effectiveTooltip: !enabled && disabledReason.length ? disabledReason : tooltip
    property string iconName
    // Public delegate hook keeps keyboard access while mouse actions retain
    // the scientific editor's focus, matching the other ribbon controls.
    displayComponent: Component {
        AppRibbonButton {
            action: command
            text: command.text
            iconName: command.iconName
            tip: command.effectiveTooltip
        }
    }
    objectName: commandId
    icon.width: Kirigami.Units.iconSizes.small
    icon.height: Kirigami.Units.iconSizes.small
    icon.source: iconName.length ? Qt.resolvedUrl("../icons/" + iconName + ".svg") : ""
}
