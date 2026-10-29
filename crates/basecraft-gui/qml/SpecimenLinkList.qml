import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Bounded and virtualized: only visible specimen-link editors are instantiated.
// Navigate logical rows rather than the currently instantiated delegate items.
FocusScrollView {
    id: control
    property var rows: [] // [{id, label}]
    property var specimenIds: []
    property var links: ({})
    property string emptyText: "Unlinked"
    property alias view: list
    property Item nextFocusItem: null
    activeFocusOnTab: true
    function focusRow(row, reason) {
        if (row < 0 || row >= list.count) return
        list.currentIndex = row
        list.positionViewAtIndex(row, ListView.Contain)
        list.forceLayout()
        const item = list.itemAtIndex(row)
        if (item) item.editor.forceActiveFocus(reason)
    }
    function focusLastOnBacktab(event) {
        const backward = event.key === Qt.Key_Backtab
            || (event.key === Qt.Key_Tab && (event.modifiers & Qt.ShiftModifier))
        if (backward && list.count > 0
                && !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))) {
            event.accepted = true
            focusRow(list.count - 1, Qt.BacktabFocusReason)
        } else event.accepted = false
    }
    function containsItem(item) {
        while (item) {
            if (item === control) return true
            item = item.parent
        }
        return false
    }
    function leaveList(item, reason, event) {
        // Resume Qt's ordinary chain before the list. Footer focus remains
        // AppDialog's responsibility; do not target its buttons specially.
        if (!item && reason === Qt.BacktabFocusReason) {
            item = control.nextItemInFocusChain(false)
            while (item !== control && containsItem(item)) item = item.nextItemInFocusChain(false)
        }
        if (item && item.visible && item.enabled) {
            item.forceActiveFocus(reason)
            event.accepted = true
        } else event.accepted = false
    }
    onActiveFocusChanged: if (activeFocus) Qt.callLater(() => {
        const focused = control.Window.window ? control.Window.window.activeFocusItem : null
        if (focused === control || focused === list)
            focusRow(focusReason === Qt.BacktabFocusReason ? list.count - 1 : 0, focusReason)
    })
    signal edited(string record, string specimen)
    FontMetrics { id: metrics; font: control.font }
    implicitWidth: metrics.averageCharacterWidth * 64
    implicitHeight: 200
    Layout.fillWidth: true
    Layout.fillHeight: true
    Layout.minimumHeight: metrics.height * 3
    Layout.maximumHeight: 240
    clip: true
    contentWidth: availableWidth
    ListView {
        id: list
        objectName: "specimenLinkRows"
        width: control.availableWidth
        height: control.availableHeight
        clip: true
        model: control.rows
        spacing: 4
        activeFocusOnTab: false
        keyNavigationEnabled: true
        delegate: RowLayout {
            required property var modelData
            required property int index
            property alias editor: chooser
            width: ListView.view.width
            height: implicitHeight
            spacing: 8
            Label {
                text: modelData.label
                elide: Text.ElideRight
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                ToolTip.text: text
                ToolTip.visible: tipHover.hovered
                HoverHandler { id: tipHover }
            }
            ComboBox {
                id: chooser
                objectName: "specimenLinkChoice"
                Layout.preferredWidth: metrics.averageCharacterWidth * 22
                Layout.maximumWidth: list.width * 0.55
                model: [control.emptyText].concat(control.specimenIds)
                currentIndex: Math.max(0, model.indexOf(control.links[modelData.id] || control.emptyText))
                Accessible.name: "Project specimen for " + modelData.id
                onActiveFocusChanged: if (activeFocus) {
                    list.currentIndex = index
                    list.positionViewAtIndex(index, ListView.Contain)
                }
                Keys.onPressed: event => {
                    if ((event.key !== Qt.Key_Tab && event.key !== Qt.Key_Backtab)
                            || (event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))) {
                        event.accepted = false
                        return
                    }
                    const backward = event.key === Qt.Key_Backtab || (event.modifiers & Qt.ShiftModifier)
                    const row = index + (backward ? -1 : 1)
                    const reason = backward ? Qt.BacktabFocusReason : Qt.TabFocusReason
                    event.accepted = true
                    if (row >= 0 && row < list.count) control.focusRow(row, reason)
                    else control.leaveList(backward ? null : control.nextFocusItem, reason, event)
                }
                onActivated: control.edited(modelData.id, currentIndex === 0 ? "" : currentText)
            }
        }
    }
}
