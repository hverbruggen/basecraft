import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Templates as Templates

// Keep the focused field visible when a form is taller than its dialog.
ScrollView {
    id: scroll
    function revealFocus() {
        const window = scroll.Window.window
        if (!window || !contentItem || contentItem.contentY === undefined) return
        let field = window.activeFocusItem
        let control = field
        let inside = false
        while (field) {
            if (field === scroll) { inside = true; break }
            if (!(control instanceof Templates.Control) && field instanceof Templates.Control)
                control = field
            field = field.parent
        }
        if (!inside || !control) return
        const flick = contentItem
        const point = control.mapToItem(flick.contentItem, 0, 0)
        const bottom = point.y + Math.min(control.height, flick.height)
        let y = flick.contentY
        if (point.y < y) y = point.y
        else if (bottom > y + flick.height) y = bottom - flick.height
        flick.contentY = Math.max(0, Math.min(y, flick.contentHeight - flick.height))
    }
    property Connections focusConnection: Connections {
        target: scroll.Window.window
        function onActiveFocusItemChanged() { Qt.callLater(scroll.revealFocus) }
    }
    onHeightChanged: Qt.callLater(revealFocus)
    // Wrapped text and newly visible rows can finish laying out after the focus
    // notification. Reveal again with the final geometry, without moving focus.
    onContentHeightChanged: Qt.callLater(revealFocus)
}
