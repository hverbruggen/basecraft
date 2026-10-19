import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Templates as T
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Control {
    id: ribbon
    property real ribbonSizeFactor: 1
    property real baseFontPixelSize: Window.window && Window.window.font ? Window.window.font.pixelSize : 12
    font.pixelSize: baseFontPixelSize * ribbonSizeFactor
    default property alias pages: stack.data
    property alias currentIndex: stack.currentIndex
    readonly property Item currentPage: stack.currentIndex >= 0 && stack.currentIndex < stack.children.length ? stack.children[stack.currentIndex] : null
    readonly property real contentWidth: viewport.contentWidth
    // The width the current page needs: its visible groups at their natural
    // widths, plus spacing. Summed here rather than read from the page's
    // row layout, whose implicit width can stay larger than its groups need.
    readonly property real pageWidth: {
        const page = currentPage
        if (!page) return 0
        let total = 0
        let count = 0
        for (const child of page.children) {
            if (!child.visible) continue
            // A stretching spacer needs no width; in the app such spacers report
            // a natural width of 200. Never read Layout.minimumWidth here: when
            // unset it queries the layout engine, which crashes while a popup
            // reparents its items. RibbonGroup sets its minimum to implicitWidth.
            total += child.Layout.fillWidth ? 0 : Math.ceil(child.implicitWidth)
            count++
        }
        return total + Math.max(0, count - 1) * (page.spacing || 0)
    }
    readonly property real viewportWidth: viewport.width
    readonly property real scrollPosition: viewport.contentX
    readonly property bool overflowing: contentWidth > viewportWidth
    implicitHeight: (currentPage ? currentPage.implicitHeight : 0) + topPadding + bottomPadding + (overflowing ? bar.implicitHeight : 0)
    padding: Kirigami.Units.smallSpacing * ribbonSizeFactor
    clip: true

    function reveal(item) {
        // Editable controls focus an internal TextInput; reveal the whole field.
        while (item.parent && !(item instanceof T.Control)) item = item.parent
        const position = item.mapToItem(viewport.contentItem, 0, 0)
        if (position.x < viewport.contentX) viewport.contentX = position.x
        else if (position.x + item.width > viewport.contentX + viewport.width)
            viewport.contentX = position.x + item.width - viewport.width
        viewport.contentX = Math.max(0, Math.min(viewport.contentX, viewport.contentWidth - viewport.width))
    }
    function revealFocused() {
        const item = ribbon.Window.window ? ribbon.Window.window.activeFocusItem : null
        if (!item || !item.visible) return
        for (let ancestor = item; ancestor; ancestor = ancestor.parent) {
            if (ancestor === stack) { ribbon.reveal(item); return }
        }
    }
    Connections {
        target: ribbon.Window.window
        function onActiveFocusItemChanged() { Qt.callLater(ribbon.revealFocused) }
    }
    Flickable {
        id: viewport
        x: ribbon.leftPadding
        y: ribbon.topPadding
        width: ribbon.availableWidth
        height: Math.max(0, ribbon.availableHeight - (ribbon.overflowing ? bar.implicitHeight : 0))
        clip: true
        contentWidth: Math.max(width, ribbon.pageWidth)
        contentHeight: height
        onContentWidthChanged: Qt.callLater(ribbon.revealFocused)
        onWidthChanged: Qt.callLater(ribbon.revealFocused)
        flickableDirection: Flickable.HorizontalFlick
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.horizontal: ScrollBar {
            id: bar
            parent: ribbon
            x: viewport.x
            y: viewport.y + viewport.height
            width: viewport.width
            policy: ScrollBar.AlwaysOn
            visible: ribbon.overflowing
            active: true
        }
        StackLayout {
            id: stack
            width: viewport.contentWidth
            height: viewport.height
            onCurrentIndexChanged: viewport.contentX = ribbon.mirrored ? Math.max(0, viewport.contentWidth - viewport.width) : 0
        }
    }
}
