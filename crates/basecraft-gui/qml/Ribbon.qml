import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Control {
    id: ribbon
    property real ribbonSizeFactor: 1
    property real symbolBandSize: 16
    property real baseFontPixelSize: Window.window && Window.window.font ? Window.window.font.pixelSize : 12
    font.pixelSize: baseFontPixelSize * ribbonSizeFactor
    default property alias pages: stack.data
    property alias currentIndex: stack.currentIndex
    readonly property Item currentPage: stack.currentIndex >= 0 && stack.currentIndex < stack.children.length ? stack.children[stack.currentIndex] : null
    readonly property real contentWidth: pageWidth
    readonly property real viewportWidth: availableWidth
    readonly property real scrollPosition: 0
    readonly property bool overflowing: pageWidth > availableWidth
    readonly property real pageWidth: currentPage ? currentPage.implicitWidth : 0
    readonly property real pageHeight: {
        let result = 0
        for (const page of stack.children) result = Math.max(result, page.implicitHeight)
        return result
    }
    implicitHeight: pageHeight + topPadding + bottomPadding
    padding: 4 * ribbonSizeFactor
    signal escapeRequested()
    property var browsingControl: null
    property var browsingGroup: null
    function groups() { return currentPage ? Array.from(currentPage.children).filter(c => c instanceof RibbonGroup && c.visible) : [] }
    function adapt() {
        const list = groups()
        list.forEach(g => g.presentation = 0)
        if (!currentPage) return
        for (const child of currentPage.children) if (child instanceof ToolSeparator) child.visible = true
        let total = () => {
            let sum = 0
            for (const child of currentPage ? currentPage.children : [])
                if (child.visible && !child.Layout.fillWidth) sum += child.implicitWidth
            return sum + Math.max(0, currentPage.children.filter(c => c.visible).length - 1) * currentPage.spacing
        }
        // Low priorities collapse first; retain common edit/navigation controls.
        if (total() > availableWidth) {
            for (const child of currentPage.children) if (child instanceof ToolSeparator) child.visible = false
        }
        const ordered = list.slice().sort((a,b) => a.collapsePriority - b.collapsePriority || list.indexOf(b) - list.indexOf(a))
        for (const stage of [1,2]) {
            for (const group of ordered) {
                if (total() <= availableWidth) break
                group.presentation = stage
            }
        }
    }
    onAvailableWidthChanged: Qt.callLater(adapt)
    onCurrentPageChanged: { browsingControl = null; Qt.callLater(adapt) }
    onRibbonSizeFactorChanged: Qt.callLater(adapt)
    Component.onCompleted: Qt.callLater(adapt)
    function browse(group, index) {
        if (!group) return
        const entries = group.entries()
        if (!entries.length) return
        browsingGroup = group
        browsingControl = entries[(index + entries.length) % entries.length]
        if (group.presentation === 2) group.open()
        browser.forceActiveFocus(Qt.TabFocusReason)
    }
    function enter(backward) {
        const list = groups().filter(g => g.entries().length)
        browse(backward ? list[list.length-1] : list[0], 0)
    }
    function moveGroup(backward) {
        const list = groups().filter(g => g.entries().length)
        browse(list[(list.indexOf(browsingGroup) + (backward ? list.length-1 : 1)) % list.length], 0)
    }
    function activate(control) {
        if (!control || !control.enabled) return
        if (control.commandAction) control.commandAction.trigger()
        else if (control.action) control.action.trigger()
        else if (control instanceof RibbonNumberField) control.edit()
        else if (control.clicked) control.clicked()
        else { control.forceActiveFocus(Qt.TabFocusReason); if (control.toggle) control.toggle() }
    }
    Item {
        id: browser
        objectName: "ribbonBrowser"
        anchors.fill: parent
        z: 2
        Accessible.role: Accessible.Button
        Accessible.name: ribbon.browsingControl ? ribbon.browsingControl.Accessible.name : "Ribbon"
        Accessible.description: ribbon.browsingControl ? (ribbon.browsingControl.effectiveTip || ribbon.browsingControl.tip || "") : ""
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Escape) { ribbon.browsingControl = null; ribbon.escapeRequested() }
            else if (event.key === Qt.Key_Tab || event.key === Qt.Key_Backtab) ribbon.moveGroup(event.key === Qt.Key_Backtab || !!(event.modifiers & Qt.ShiftModifier))
            else if ([Qt.Key_Left, Qt.Key_Right, Qt.Key_Up, Qt.Key_Down].includes(event.key)) {
                const entries = ribbon.browsingGroup.entries()
                ribbon.browse(ribbon.browsingGroup, entries.indexOf(ribbon.browsingControl) + ([Qt.Key_Left, Qt.Key_Up].includes(event.key) ? -1 : 1))
            } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) ribbon.activate(ribbon.browsingControl)
            else { event.accepted = false; return }
            event.accepted = true
        }
        Rectangle {
            parent: Overlay.overlay
            z: 999
            readonly property var targetControl: ribbon.browsingControl
            readonly property point position: targetControl ? targetControl.mapToItem(parent, 0, 0) : Qt.point(0,0)
            visible: browser.activeFocus && targetControl !== null
            x: position.x; y: position.y
            width: targetControl ? targetControl.width : 0
            height: targetControl ? targetControl.height : 0
            color: "transparent"
            border.width: 2
            border.color: ribbon.palette.highlight
        }
        ToolTip.visible: activeFocus && Accessible.description.length > 0
        ToolTip.text: Accessible.description
        ToolTip.delay: 0
    }
    StackLayout {
        id: stack
        x: ribbon.leftPadding; y: ribbon.topPadding
        width: ribbon.availableWidth; height: ribbon.availableHeight
    }
}
