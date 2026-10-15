import QtQuick
import QtQuick.Controls.Basic

// Shared scroll container for editors, confirmations and explanatory content.
AppDialog {
    id: dialog
    property Item body
    FontMetrics { id: metrics; font: dialog.font }
    onBodyChanged: if (body) body.parent = flick.contentItem
    Binding { target: dialog.body; property: "width"; value: flick.width; when: dialog.body !== null }
    contentItem: FocusScrollView {
        id: scroll
        clip: true
        implicitWidth: Math.min(dialog.body ? dialog.body.implicitWidth : 0,
            metrics.averageCharacterWidth * 76)
        contentWidth: availableWidth
        implicitHeight: Math.min(dialog.body ? dialog.body.implicitHeight : 0,
            Math.max(48, (Overlay.overlay ? Overlay.overlay.height : 760) - 160))
        Flickable {
            id: flick
            contentWidth: width
            contentHeight: dialog.body ? dialog.body.implicitHeight : 0
            boundsBehavior: Flickable.StopAtBounds
        }
    }
}
