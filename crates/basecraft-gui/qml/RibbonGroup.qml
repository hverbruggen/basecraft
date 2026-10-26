import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigami.layouts as KirigamiLayouts

Control {
    id: group
    RibbonMetrics { id: metrics; control: group }
    property string title
    property alias actions: toolbar.actions
    // Controls in this slot keep their natural size, including editable fields.
    default property alias controls: row.data
    readonly property real captionY: caption.y
    readonly property real bodyCenterY: body.height / 2
    // Layouts assign whole pixels; round up so fractional font metrics fit.
    implicitWidth: Math.ceil(Math.max(row.implicitWidth, caption.implicitWidth))
    implicitHeight: row.implicitHeight + caption.implicitHeight + Kirigami.Units.smallSpacing * metrics.factor
    Layout.fillHeight: true
    Layout.minimumWidth: implicitWidth
    padding: 0
    Item {
        id: body
        width: parent.width
        height: Math.max(0, parent.height - caption.implicitHeight - Kirigami.Units.smallSpacing * metrics.factor)
        RowLayout {
            id: row
            anchors.centerIn: parent
            width: parent.width
            height: implicitHeight
            spacing: metrics.groupSpacing
            Kirigami.ActionToolBar {
                id: toolbar
                // Keep fallback style minimums compact while reserving the full
                // shared tile, including its caption and clickable padding.
                implicitHeight: Math.max(24 * metrics.factor, toolbarMetrics.height + 6 * metrics.factor, metrics.tileHeight)
                heightMode: KirigamiLayouts.ToolBarLayout.AlwaysFill
                FontMetrics { id: toolbarMetrics; font: toolbar.font }
                visible: actions.length > 0
                Layout.fillWidth: false
                Layout.preferredWidth: implicitWidth
                Layout.minimumWidth: implicitWidth
                // The page scrolls as a whole; fields never disappear into menus.
                overflowIconName: Qt.resolvedUrl("../icons/more.svg").toString()
            }
        }
    }
    Label {
        id: caption
        y: parent.height - height
        width: parent.width
        height: implicitHeight
        text: group.title
        horizontalAlignment: Text.AlignHCenter
        font.pixelSize: Math.max(9, group.font.pixelSize / metrics.factor * 0.85) * metrics.factor
        opacity: 0.65
    }
}
