import QtQuick
import org.kde.kirigami as Kirigami

Kirigami.Action {
    id: command
    required property string commandId
    property var customization: ({ shortcuts: {}, ribbon_hidden: [] })
    property string defaultShortcut: ""
    shortcut: customization.shortcuts && customization.shortcuts[commandId] !== undefined
        ? customization.shortcuts[commandId] : defaultShortcut
    visible: !customization.ribbon_hidden || customization.ribbon_hidden.indexOf(commandId) < 0
    property string iconName
    // Public delegate hook keeps keyboard access while mouse actions retain
    // the scientific editor's focus, matching the other ribbon controls.
    displayComponent: Component {
        AppRibbonButton {
            action: command
            text: command.text
            iconName: command.iconName
            tip: command.tooltip
        }
    }
    objectName: commandId
    icon.width: Kirigami.Units.iconSizes.small
    icon.height: Kirigami.Units.iconSizes.small
    icon.source: iconName.length ? Qt.resolvedUrl("../icons/" + iconName + ".svg") : ""
}
