import QtQuick
import QtQuick.Controls.Basic

// A small captioned number box for the ribbon, such as a font size.
Item {
    id: field
    property alias label: caption.text
    property alias from: box.from
    property alias to: box.to
    property real value: 0
    property bool fractional: false
    property string disabledReason
    readonly property string effectiveTip: !enabled && disabledReason.length ? disabledReason : tip
    HoverHandler { id: tipHover }
    ToolTip.text: effectiveTip
    ToolTip.visible: effectiveTip.length > 0 && (tipHover.hovered || box.activeFocus)
    ToolTip.delay: 400
    property string tip
    signal edited(real value)
    // As tall as a ribbon button, so captions across the ribbon line up.
    RibbonMetrics { id: metrics; control: field }
    implicitHeight: metrics.tileHeight
    implicitWidth: Math.max(box.implicitWidth, caption.implicitWidth + box.stepperSize)
    Label {
        id: caption
        z: 1
        x: box.mirrored ? box.stepperSize : 0
        width: field.width - box.stepperSize
        horizontalAlignment: Text.AlignHCenter
        font.pixelSize: metrics.captionPixelSize
        opacity: field.enabled ? 1 : 0.45
    }
    AppSpinBox {
        id: box
        objectName: field.objectName + "Box"
        ribbonMode: true
        editable: true
        anchors.fill: parent
        topPadding: caption.implicitHeight
        font.pixelSize: metrics.baseFontPixelSize * metrics.factor
        widthSample: contentItem.text
        value: Math.round(field.value)
        valueFromText: (text, locale) => field.fractional ? Math.round(field.value) : Number.fromLocaleString(locale, text)
        contentItem: TextInput {
            text: field.fractional ? String(field.value) : box.textFromValue(box.value, box.locale)
            font: box.font
            color: box.enabled ? box.palette.text : box.palette.placeholderText
            selectionColor: box.palette.highlight
            selectedTextColor: box.palette.highlightedText
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            selectByMouse: true
            readOnly: !box.editable
            leftPadding: 4; rightPadding: 4
            validator: field.fractional ? fractionValidator : box.validator
            Keys.onEscapePressed: {
                text = field.fractional ? String(field.value) : box.textFromValue(box.value, box.locale)
                focus = false
            }
            onEditingFinished: {
                if (acceptableInput) field.edited(field.fractional ? Number(text) : box.valueFromText(text, box.locale))
                text = Qt.binding(() => field.fractional ? String(field.value) : box.textFromValue(box.value, box.locale))
            }
        }
        DoubleValidator { id: fractionValidator; bottom: 0.000001; top: box.to; notation: DoubleValidator.StandardNotation; locale: "C" }
        // SpinBox supplies integer steps; retain the fractional part of zoom.
        onValueModified: field.edited(field.fractional
            ? Math.max(0.000001, Math.min(to, field.value + value - Math.round(field.value))) : value)
        Accessible.name: field.tip.length ? field.tip : caption.text
        Accessible.description: field.effectiveTip
    }
}
