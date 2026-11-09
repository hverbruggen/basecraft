import QtQuick
import QtQuick.Controls.Basic

// Fit ribbon menus to their labels, without Basic's fixed minimum width.
Menu {
    id: menu
    Connections {
        target: menu
        function onOpened() {
            const host = menu.parent ? menu.parent.Window.window : null
            if (host && host.ribbonNavigation) host.ribbonNavigation.menuOpened(menu)
        }
    }
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
