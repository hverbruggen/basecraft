import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Templates as T
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Control {
    id: group
    RibbonMetrics { id: metrics; control: group }
    property string disabledReason
    property int presentation: 0
    property int collapsePriority: ["Annotation", "Rows", "Zoom", "Issues", "File", "GenBank"].includes(title) ? 100 : ["Align", "Root", "Calls", "Figure"].includes(title) ? 50 : 0
    readonly property real compactGroupWidth: Math.ceil(Math.max(bodyWidth(true), caption.implicitWidth))
    readonly property real collapsedWidth: 24 * metrics.factor
    readonly property real expandedWidth: Math.ceil(Math.max(bodyWidth(false), caption.implicitWidth))
    onCompactGroupWidthChanged: if (metrics.ribbon && metrics.ribbon.adapt) Qt.callLater(metrics.ribbon.adapt)
    onExpandedWidthChanged: if (metrics.ribbon && metrics.ribbon.adapt) Qt.callLater(metrics.ribbon.adapt)
    function bodyWidth(compact) {
        const items = Array.from(row.children).filter(c => c === toolbar ? toolbar.actions.length > 0 : c.commandAction ? c.commandAction.visible : c.action ? c.action.visible : true)
        return items.reduce((sum, c) => sum + (c === toolbar ? toolbarWidth(compact) : c.ribbonControl === true ? (compact ? c.compactWidth : c.expandedWidth) : c.implicitWidth), 0) + Math.max(0, items.length - 1) * row.spacing
    }
    function toolbarWidth(compact) {
        const items = Array.from(toolbar.children).filter(c => c.ribbonControl === true && c.action && c.action.visible)
        return items.reduce((sum,c) => sum + (compact ? c.compactWidth : c.expandedWidth),0) + Math.max(0,items.length-1)*toolbar.spacing
    }
    function entries() {
        const result = []
        function visit(item) {
            if (item.ribbonControl === true || item instanceof RibbonNumberField || item instanceof T.CheckBox || item instanceof T.ComboBox || item instanceof T.SpinBox) {
                const command = item.commandAction || item.action
                if (!command || command.visible !== false) result.push(item)
                return
            }
            for (const child of item.children || []) visit(child)
        }
        visit(row)
        return result
    }
    onPresentationChanged: if (presentation !== 2) close()
    property alias collapseControl: collapseButton
    function close() { overflow.close() }
    function open() { if (presentation === 2) overflow.open() }
    property string title
    property alias actions: toolbar.actions
    // Controls in this slot keep their natural size, including editable fields.
    default property alias controls: row.data
    readonly property real captionY: caption.y
    readonly property real bodyCenterY: body.height / 2
    // Layouts assign whole pixels; round up so fractional font metrics fit.
    implicitWidth: presentation === 2 ? 24 * metrics.factor : Math.ceil(Math.max(bodyWidth(presentation === 1), caption.implicitWidth))
    implicitHeight: Math.max(row.implicitHeight, metrics.tileHeight) + caption.implicitHeight + Kirigami.Units.smallSpacing * metrics.factor
    Layout.fillHeight: true
    Layout.minimumWidth: implicitWidth
    padding: 0
    Item {
        id: body
        readonly property var ribbonGroupOwner: group
        function escapeRequested() {
            group.close()
            if (metrics.ribbon && metrics.ribbon.escapeRequested) metrics.ribbon.escapeRequested()
        }
        readonly property font font: group.font
        readonly property real ribbonSizeFactor: metrics.factor
        readonly property real baseFontPixelSize: metrics.baseFontPixelSize
        readonly property real symbolBandSize: metrics.ribbon ? metrics.ribbon.symbolBandSize : 16
        parent: overflow.visible ? overflow.contentItem : group
        visible: group.presentation !== 2 || overflow.visible
        enabled: group.enabled
        width: overflow.visible ? group.expandedWidth : group.width
        height: Math.max(row.implicitHeight, metrics.tileHeight)
        RowLayout {
            id: row
            anchors.centerIn: parent
            width: implicitWidth
            height: implicitHeight
            spacing: metrics.groupSpacing
            RowLayout {
                id: toolbar
                property var actions: []
                spacing: metrics.groupSpacing
                visible: actions.length > 0
                Repeater {
                    model: toolbar.actions
                    AppRibbonButton {
                        required property var modelData
                        action: modelData
                        visible: modelData.visible
                        iconName: modelData.iconName
                        tip: modelData.effectiveTooltip
                    }
                }
            }
        }
    }
    AppRibbonButton {
        id: collapseButton
        visible: group.presentation === 2
        width: group.width
        text: ""
        iconName: "more"
        tip: group.disabledReason || group.title
        onClicked: group.open()
    }
    Popup {
        id: overflow
        objectName: "ribbonGroupOverflow"
        y: group.height
        width: group.expandedWidth + leftPadding + rightPadding
        height: Math.max(row.implicitHeight, metrics.tileHeight) + topPadding + bottomPadding
        onClosed: if (collapseButton.visible && !group.Window.window.activeFocusItem) collapseButton.forceActiveFocus()
    }
    Label {
        id: caption
        y: parent.height - height
        width: parent.width
        height: implicitHeight
        text: group.title
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignHCenter
        font.pixelSize: Math.max(9, group.font.pixelSize / metrics.factor * 0.85) * metrics.factor
        opacity: 0.65
    }
}
