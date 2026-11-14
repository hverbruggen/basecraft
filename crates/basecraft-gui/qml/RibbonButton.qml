import QtQuick

// A ribbon button for one command. Its caption, tip, keys, icon and why it is
// unavailable come from the main window's command list.
AppCommandButton {
    readonly property var appWindow: Window.window && Window.window.commandList ? Window.window : null
    text: appWindow ? appWindow.commandList.caption(commandId) : ""
    tip: appWindow ? appWindow.commandList.tip(commandId) : ""
    customization: appWindow ? appWindow.customization : ({shortcuts: {}, ribbon_hidden: []})
    disabledReason: appWindow ? appWindow.commandList.disabled_reason(commandId, appWindow.commandContext) : ""
    hintScope: appWindow ? appWindow.commandList.scope(commandId) : ""
    defaultKeyHint: appWindow ? appWindow.commandList.key(commandId) : ""
    enabled: appWindow !== null && appWindow.current !== null
    iconName: appWindow ? appWindow.commandList.icon(commandId) : ""
    // A button outside the main window cannot register; the ribbon journey
    // fails on this line.
    Component.onCompleted: if (appWindow) appWindow.registerRibbonCommand(commandAction)
        else console.warn("RibbonButton failed to register " + commandId)
}
