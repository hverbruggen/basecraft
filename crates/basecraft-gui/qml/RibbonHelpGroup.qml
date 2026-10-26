import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The same compact, caption-free utility group on every ribbon page.
Control {
    id: root
    required property Action shortcutsAction
    required property Action aboutAction
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
            columns: 2
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
                icon.source: Qt.resolvedUrl("../branding/icon-" + (root.darkBackground ? "dark" : "light") + ".svg")
                icon.color: "transparent"
                description: "About Basecraft"
            }
            Item {
                implicitWidth: root.buttonSide
                implicitHeight: root.buttonSide
                UtilityButton {
                    objectName: "ribbonHelpDocumentation"
                    anchors.fill: parent
                    iconName: "text-representation"
                    description: "Documentation coming soon"
                    enabled: false
                }
                HoverHandler { id: documentationHover }
                ToolTip.text: "Documentation coming soon"
                ToolTip.visible: documentationHover.hovered
                ToolTip.delay: 400
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
