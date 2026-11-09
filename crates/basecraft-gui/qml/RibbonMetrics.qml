import QtQuick

// Read the nearest ribbon's scale without changing the application/window font.
// Walking visual parents also reaches buttons created by ActionToolBar delegates.
QtObject {
    id: metrics
    required property Item control
    readonly property var ribbon: {
        for (let item = control.parent; item; item = item.parent) {
            if (item.ribbonSizeFactor !== undefined) return item
        }
        return null
    }
    readonly property real factor: ribbon ? ribbon.ribbonSizeFactor : 1
    readonly property real baseFontPixelSize: ribbon ? ribbon.baseFontPixelSize
        : control.Window.window && control.Window.window.font ? control.Window.window.font.pixelSize : 12
    readonly property real captionPixelSize: Math.max(9, Math.round(baseFontPixelSize * 0.8)) * factor
    readonly property font sourceFont: control.font !== undefined ? control.font
        : ribbon ? ribbon.font : Qt.application.font
    readonly property FontMetrics captionMetrics: FontMetrics {
        font.family: metrics.sourceFont.family
        font.weight: metrics.sourceFont.weight
        font.italic: metrics.sourceFont.italic
        font.styleName: metrics.sourceFont.styleName
        font.pixelSize: metrics.captionPixelSize
    }
    readonly property real symbolHeight: Math.max((ribbon ? ribbon.symbolBandSize : 16) * factor, Math.ceil(captionMetrics.height))
    // Shared tile: symbol band, caption, two-pixel gap and six-pixel padding.
    // ActionToolBar must reserve this height rather than constraining its tiles.
    readonly property real tileHeight: symbolHeight + Math.ceil(captionMetrics.height) + 8 * factor
    readonly property real groupSpacing: 5 * factor
}
