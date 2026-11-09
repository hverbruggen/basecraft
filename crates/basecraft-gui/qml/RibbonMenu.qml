import QtQuick
import QtQuick.Controls.Basic

// Fit ribbon menus to their labels, without Basic's fixed minimum width.
Menu {
    onOpened: if (Window.window && Window.window.ribbonNavigation) Window.window.ribbonNavigation.menuOpened(this)
    property real sizeFactor: 1
    implicitWidth: {
        let widest = 0
        for (let i = 0; i < count; i++) {
            const item = itemAt(i)
            if (item) widest = Math.max(widest, item.implicitWidth)
        }
        return widest + leftPadding + rightPadding
    }
}
