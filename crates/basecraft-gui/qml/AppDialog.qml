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
    implicitWidth: Math.max(implicitContentWidth + leftPadding + rightPadding,
        implicitHeaderWidth, implicitFooterWidth)
    readonly property real maximumDialogWidth: hostWindow ? hostWindow.width * 0.9 : 1080
    readonly property real maximumContentWidth: Math.max(0, maximumDialogWidth - leftPadding - rightPadding)
    width: Math.max(0, Math.min(implicitWidth, maximumDialogWidth))
    height: Math.max(0, Math.min(implicitHeight,
        Overlay.overlay ? Overlay.overlay.height - 32 : implicitHeight))
    // Dialog's Escape handling calls reject(), including for Close-only dialogs.
    // Keep it local to the focused popup so nested combo menus close first.
    closePolicy: Popup.CloseOnEscape

    // Basic's header/footer backgrounds can cover the outline, and its default
    // button box distributes two buttons across the entire dialog.
    header: Label {
        text: dialog.title
        visible: text.length > 0
        font.family: dialog.font.family
        font.pixelSize: dialog.font.pixelSize
        font.bold: true
        wrapMode: Text.Wrap
        padding: dialog.padding
        leftPadding: dialog.leftPadding
        rightPadding: dialog.rightPadding
    }
    footer: DialogButtonBox {
        id: buttons
        readonly property real naturalButtonsWidth: {
            let total = Math.max(0, count - 1) * spacing
            for (let i = 0; i < count; ++i) total += itemAt(i).implicitWidth
            return total
        }
        readonly property real widestButton: {
            let widest = 0
            for (let i = 0; i < count; ++i) widest = Math.max(widest, itemAt(i).implicitWidth)
            return widest
        }
        readonly property bool stacked: availableWidth < naturalButtonsWidth
        visible: count > 0
        alignment: Qt.AlignRight
        spacing: 8
        padding: dialog.padding
        leftPadding: dialog.leftPadding
        rightPadding: dialog.rightPadding
        implicitWidth: naturalButtonsWidth + leftPadding + rightPadding
        implicitHeight: contentItem.implicitHeight + topPadding + bottomPadding
        contentWidth: stacked ? widestButton : naturalButtonsWidth
        contentHeight: contentItem.implicitHeight
        delegate: AppButton { width: Math.min(implicitWidth, buttons.availableWidth) }
        contentItem: ListView {
            implicitWidth: buttons.contentWidth
            implicitHeight: {
                let total = 0
                for (let i = 0; i < buttons.count; ++i) {
                    const height = buttons.itemAt(i).implicitHeight
                    total = buttons.stacked ? total + height : Math.max(total, height)
                }
                return total + (buttons.stacked ? Math.max(0, buttons.count - 1) * spacing : 0)
            }
            model: buttons.contentModel
            spacing: buttons.spacing
            orientation: buttons.stacked ? ListView.Vertical : ListView.Horizontal
            boundsBehavior: Flickable.StopAtBounds
            interactive: false
        }
        background: Item {}
    }

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
        // A nested dialog may have opened before this deferred callback ran.
        // Only the popup that still owns focus may choose its initial control.
        if (!opened || !activeFocus) return
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
        function onOpened() {
            Qt.callLater(dialog.focusInitialControl)
            if (dialog.hostWindow && dialog.hostWindow.ribbonNavigation)
                Qt.callLater(() => dialog.hostWindow.ribbonNavigation.registerDialog(dialog))
        }
        function onClosed() {
            if (dialog.hostWindow && dialog.hostWindow.ribbonNavigation)
                dialog.hostWindow.ribbonNavigation.unregisterDialog(dialog)
            const target = dialog.usable(dialog.returnFocusItem) ? dialog.returnFocusItem
                : dialog.previousFocusItem
            // An accept/reject handler can open the next popup before closed
            // is delivered. Do not restore focus underneath that new popup.
            let focused = dialog.hostWindow ? dialog.hostWindow.activeFocusItem : null
            const overlay = dialog.parent ? dialog.parent.Overlay.overlay : null
            while (focused && focused !== overlay) focused = focused.parent
            if (!focused && dialog.usable(target)) target.forceActiveFocus(Qt.PopupFocusReason)
            dialog.previousFocusItem = null
        }
    }
}
