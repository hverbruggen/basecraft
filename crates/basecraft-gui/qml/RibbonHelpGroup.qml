import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The same compact, caption-free utility group on every ribbon page.
Control {
    id: root
    required property Action shortcutsAction
    required property Action aboutAction
    property Action preferencesAction
    property Action exportSvgAction
    property Action exportPngAction
    property color backgroundColor: "white"
    readonly property bool darkBackground: 0.2126 * backgroundColor.r
        + 0.7152 * backgroundColor.g + 0.0722 * backgroundColor.b < 0.5
    RibbonMetrics { id: metrics; control: root }
    readonly property real buttonSide: Math.ceil(Math.max(20 * metrics.factor, metrics.symbolHeight + 4 * metrics.factor))
    padding: 0
    implicitWidth: contents.implicitWidth
    implicitHeight: contents.implicitHeight
    Layout.minimumWidth: implicitWidth
    Layout.fillHeight: true

    component UtilityButton: AppIconButton {
        implicitWidth: root.buttonSide
        implicitHeight: root.buttonSide
        padding: 2 * metrics.factor
        icon.width: 16 * metrics.factor
        icon.height: 16 * metrics.factor
        focusPolicy: Qt.TabFocus
        ToolTip.visible: (hovered || activeFocus) && description.length > 0
        ToolTip.delay: 400
    }

    contentItem: RowLayout {
        id: contents
        spacing: 2 * metrics.factor
        ToolSeparator {
            objectName: "ribbonHelpSeparator"
            orientation: Qt.Vertical
            Layout.fillHeight: true
        }
        GridLayout {
            columns: 3
            rowSpacing: 2 * metrics.factor
            columnSpacing: 2 * metrics.factor
            UtilityButton {
                objectName: "ribbonHelpShortcuts"
                action: root.shortcutsAction
                iconName: "keyboard"
                description: "Keyboard shortcuts for this tab"
            }
            UtilityButton {
                objectName: "ribbonHelpAbout"
                action: root.aboutAction
                iconName: "basecraft-monochrome"
                description: "About Basecraft"
            }
            UtilityButton {
                objectName: "ribbonHelpPreferences"
                action: root.preferencesAction
                iconName: "settings"
                description: "Preferences"
            }
            UtilityButton {
                objectName: "ribbonHelpDocumentation"
                iconName: "book"
                description: "Learn: documentation coming soon"
                enabled: false
            }
            UtilityButton {
                id: exportButton
                objectName: "ribbonFigureExport"
                iconName: "figure-export"
                description: "Export figure"
                enabled: !!root.exportSvgAction && root.exportSvgAction.enabled
                Accessible.role: Accessible.ButtonMenu
                onClicked: exportMenu.popup(exportButton, 0, height)
                Menu {
                    id: exportMenu
                    MenuItem { action: root.exportSvgAction; text: "Export SVG" }
                    MenuItem { action: root.exportPngAction; text: "Export PNG" }
                }
            }
            Item {
                objectName: "ribbonHelpReserved"
                implicitWidth: root.buttonSide
                implicitHeight: root.buttonSide
                Accessible.ignored: true
            }
        }
    }
}
