import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The same compact, caption-free utility group on every ribbon page.
Control {
    id: root
    required property Action shortcutsAction
    required property Action aboutAction
    property Action methodsAction
    property Action learnAction
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
        HoverHandler { id: utilityHover }
        Accessible.description: ToolTip.text
        ToolTip.text: action && action.effectiveTooltip !== undefined ? action.effectiveTooltip : description
        ToolTip.visible: (utilityHover.hovered || activeFocus) && ToolTip.text.length > 0
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
            Item {
                implicitWidth: root.buttonSide
                implicitHeight: root.buttonSide
                UtilityButton {
                    id: methodsButton
                    objectName: "ribbonMethods"
                    anchors.fill: parent
                    action: root.methodsAction
                    iconName: "methods-export"
                    description: "Methods"
                    ToolTip.text: root.methodsAction ? root.methodsAction.tooltip : "Open a document to read its methods"
                }
                // The parent remains hoverable when the action is disabled.
                HoverHandler { id: methodsHover }
                ToolTip.visible: methodsHover.hovered && !methodsButton.enabled
                ToolTip.text: methodsButton.ToolTip.text
                ToolTip.delay: 400
            }
            UtilityButton {
                objectName: "ribbonHelpShortcuts"
                action: root.shortcutsAction
                iconName: "keyboard"
                description: "Keyboard shortcuts for this tab"
            }
            UtilityButton {
                objectName: "ribbonHelpAbout"
                action: root.aboutAction
                iconName: "basecraft-colour"
                icon.color: "transparent"
                icon.width: 20 * metrics.factor
                icon.height: 20 * metrics.factor
                padding: 0
                description: "About Basecraft"
            }
            UtilityButton {
                id: exportButton
                objectName: "ribbonFigureExport"
                iconName: "figure-export"
                description: "Export figure"
                ToolTip.text: enabled ? "Export this view as SVG or PNG" : "Open a document to export its figure"
                enabled: !!root.exportSvgAction && root.exportSvgAction.enabled
                Accessible.role: Accessible.ButtonMenu
                onClicked: exportMenu.popup(exportButton, 0, height)
                RibbonMenu {
                    id: exportMenu
                    sizeFactor: metrics.factor
                    RibbonMenuItem { sizeFactor: metrics.factor; action: root.exportSvgAction; text: "Export SVG" }
                    RibbonMenuItem { sizeFactor: metrics.factor; action: root.exportPngAction; text: "Export PNG" }
                }
            }
            UtilityButton {
                objectName: "ribbonHelpDocumentation"
                action: root.learnAction
                iconName: "book"
                description: "Manual"
                ToolTip.text: "Manual pages are not written yet"
            }
            UtilityButton {
                objectName: "ribbonHelpPreferences"
                action: root.preferencesAction
                iconName: "settings"
                icon.color: root.darkBackground ? palette.buttonText : "#666666"
                description: "Preferences"
            }
        }
    }
}
