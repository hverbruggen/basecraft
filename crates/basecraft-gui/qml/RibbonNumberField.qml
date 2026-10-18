import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// A small captioned number box for the ribbon, such as a font size.
ColumnLayout {
    id: field
    property alias label: caption.text
    property alias from: box.from
    property alias to: box.to
    property int value: 0
    property string tip
    signal edited(int value)
    // As tall as a ribbon button, so captions across the ribbon line up.
    RibbonMetrics { id: metrics; control: field }
    implicitHeight: metrics.tileHeight
    spacing: 0
    Label {
        id: caption
        Layout.alignment: Qt.AlignHCenter
        font.pixelSize: metrics.captionPixelSize
        opacity: field.enabled ? 1 : 0.45
    }
    AppSpinBox {
        id: box
        objectName: field.objectName + "Box"
        editable: true
        Layout.fillHeight: true
        value: field.value
        onValueModified: field.edited(value)
        Accessible.name: field.tip.length ? field.tip : caption.text
        ToolTip.text: field.tip
        ToolTip.visible: field.tip.length > 0 && hovered
        ToolTip.delay: 400
    }
}
