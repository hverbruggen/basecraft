import QtQuick
import QtQuick.Controls.Basic

// Dialogs declare their first meaningful control; the base owns focus lifetime.
Dialog {
    id: dialog
    property Item initialFocusItem: null
    property Item returnFocusItem: null
    property Item previousFocusItem: null
    readonly property var hostWindow: parent ? parent.Window.window : null
    modal: true
    focus: true
    anchors.centerIn: Overlay.overlay
    implicitWidth: 680
    width: Math.max(0, Math.min(implicitWidth,
        Overlay.overlay ? Overlay.overlay.width - 32 : implicitWidth))
    height: Math.max(0, Math.min(implicitHeight,
        Overlay.overlay ? Overlay.overlay.height - 32 : implicitHeight))
    closePolicy: Popup.CloseOnEscape

    function usable(item) {
        return item && item.visible && item.enabled
    }

    function firstTabStop(item) {
        if (!usable(item)) return null
        if (item.activeFocusOnTab) return item
        for (let child of item.children) {
            const found = firstTabStop(child)
            if (found) return found
        }
        return null
    }

    function focusInitialControl() {
        if (!opened) return
        const safeCancel = (standardButtons & Dialog.Discard) && (standardButtons & Dialog.Cancel)
            ? standardButton(Dialog.Cancel) : null
        const target = usable(initialFocusItem) ? initialFocusItem
            : safeCancel || firstTabStop(contentItem) || firstTabStop(footer)
        if (target) target.forceActiveFocus(Qt.PopupFocusReason)
        else forceActiveFocus(Qt.PopupFocusReason)
    }

    // Connections keep these guarantees when a derived dialog has onOpened.
    Connections {
        target: dialog
        function onAboutToShow() {
            dialog.previousFocusItem = dialog.hostWindow ? dialog.hostWindow.activeFocusItem : null
        }
        function onOpened() { Qt.callLater(dialog.focusInitialControl) }
        function onClosed() {
            const target = dialog.usable(dialog.returnFocusItem) ? dialog.returnFocusItem
                : dialog.previousFocusItem
            if (dialog.usable(target)) target.forceActiveFocus(Qt.PopupFocusReason)
            dialog.previousFocusItem = null
        }
    }
}
