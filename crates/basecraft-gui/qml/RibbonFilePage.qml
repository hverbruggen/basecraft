import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// File: the project, saving and the document record.
RibbonPage {
    // The main window: the open document and the window's own functions.
    required property var app
    required property var combinePicker
    required property var projectCommand
    required property var newSequenceCommand
    required property var saveCommand
    required property var saveAsCommand
    required property var historyCommand
    required property var methodsCommand
    required property var recoverCommand
    spacing: 6 * app.ribbonSizeFactor
    RibbonGroup {
        title: "Project"
        AppRibbonButton { action: projectCommand; visible: projectCommand.visible; text: app.commandList.ribbon_label(projectCommand.commandId); iconName: "project"; tip: projectCommand.effectiveTooltip }
        AppRibbonButton {
            id: recentProjectButton
            property string keyHint: "RC"
            text: "Recent"
            iconName: "project"
            tip: "Open a recent project"
            Accessible.role: Accessible.ButtonMenu
            onClicked: recentProjectMenu.popup(recentProjectButton, 0, height)
            RibbonMenu {
                id: recentProjectMenu
                objectName: "recentProjectMenu"
                sizeFactor: app.ribbonSizeFactor
                RibbonMenuItem { sizeFactor: app.ribbonSizeFactor; text: "No recent projects"; enabled: false; visible: app.preferences.recent_projects.length === 0 }
                Repeater {
                    model: app.preferences.recent_projects
                    RibbonMenuItem { sizeFactor: app.ribbonSizeFactor;
                        required property string modelData
                        text: app.fileName(modelData)
                        ToolTip.text: modelData
                        ToolTip.visible: hovered
                        onTriggered: app.selectProject(modelData)
                    }
                }
            }
        }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "File"
        AppRibbonButton { action: newSequenceCommand; visible: newSequenceCommand.visible; iconName: "new-file"; tip: newSequenceCommand.effectiveTooltip }
        AppRibbonButton { action: saveCommand; visible: saveCommand.visible; iconName: "save"; tip: saveCommand.effectiveTooltip }
        AppRibbonButton { action: saveAsCommand; visible: saveAsCommand.visible; iconName: "save-as"; tip: saveAsCommand.effectiveTooltip }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Species workspaces"
        RibbonButton { commandId: "species.combine-project"; iconName: "hypothesis-combine"; enabled: true; onInvoked: combinePicker.dialog().open() }
    }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup { title: "Document record"; actions: [historyCommand, methodsCommand] }
    ToolSeparator { Layout.fillHeight: true }
    RibbonGroup {
        title: "Recovery"
        AppRibbonButton { action: recoverCommand; visible: recoverCommand.visible; iconName: "history"; tip: recoverCommand.effectiveTooltip }
    }
    Item { Layout.fillWidth: true }
    PageHelpGroup {}
}
