import QtQuick
import QtQuick.Controls.Basic

SpinBox {
    id: spin
    FontMetrics { id: metrics; font: spin.font }
    readonly property real stepperSize: Math.max(24, Math.ceil(metrics.height) + 8)
    // Reserve room for every valid value, including locale grouping separators.
    implicitWidth: Math.ceil(Math.max(metrics.advanceWidth(textFromValue(from, locale)),
        metrics.advanceWidth(textFromValue(to, locale)))) + 12 + 2 * stepperSize
    implicitHeight: Math.max(stepperSize, implicitContentHeight)
    up.indicator: AppControlBackground {
        control: ({palette: spin.palette, enabled: spin.enabled && spin.up.enabled,
            activeFocus: spin.visualFocus, pressed: spin.up.pressed, hovered: spin.up.hovered})
        x: spin.mirrored ? 0 : spin.width - width
        implicitWidth: spin.stepperSize
        implicitHeight: spin.stepperSize
        height: spin.height
        AppIcon { anchors.centerIn: parent; name: "add"; size: Math.max(12, Math.ceil(metrics.height)); color: spin.palette.buttonText }
    }
    down.indicator: AppControlBackground {
        control: ({palette: spin.palette, enabled: spin.enabled && spin.down.enabled,
            activeFocus: spin.visualFocus, pressed: spin.down.pressed, hovered: spin.down.hovered})
        x: spin.mirrored ? spin.width - width : 0
        implicitWidth: spin.stepperSize
        implicitHeight: spin.stepperSize
        height: spin.height
        AppIcon { anchors.centerIn: parent; name: "minus"; size: Math.max(12, Math.ceil(metrics.height)); color: spin.palette.buttonText }
    }
}
