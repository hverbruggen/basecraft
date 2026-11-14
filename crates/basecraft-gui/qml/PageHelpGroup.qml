import QtQuick

// The same last group on every ribbon page, wired to the main window's commands.
RibbonHelpGroup {
    readonly property var appWindow: Window.window && Window.window.commandList ? Window.window : null
    methodsAction: appWindow ? appWindow.commandFor("provenance.methods") : null
    shortcutsAction: appWindow ? appWindow.commandFor("help.context-shortcuts") : null
    aboutAction: appWindow ? appWindow.commandFor("help.about") : null
    learnAction: appWindow ? appWindow.commandFor("help.learn") : null
    preferencesAction: appWindow ? appWindow.commandFor("application.preferences") : null
    exportSvgAction: appWindow ? appWindow.commandFor("ribbon.export-an-editable-svg-of-this-view") : null
    exportPngAction: appWindow ? appWindow.commandFor("figure.export-png") : null
    backgroundColor: appWindow ? appWindow.color : "white"
}
