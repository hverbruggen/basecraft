import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Templates as T

// A single observer is forwarded keys before the focused editor handles them.
// It never grabs focus merely to watch Alt, and restores existing forwarding.
Item {
    id: hints
    required property var hostWindow
    required property var tabBar
    required property var ribbon
    property var tabKeys: ({File:"F", View:"V", Sequence:"S", Alignment:"A", Phylogeny:"P", Tree:"T", Delimitation:"D", Trace:"R", Export:"E"})
    property int level: 0
    property string prefix: ""
    property var targets: []
    property var invoker: null
    property var observed: null
    property var previousForwarding: []
    property bool altAlone: false
    property var menu: null
    property var group: null
    property string explanation: ""
    property var dialogs: []
    property var dialogTargets: []
    property bool mnemonicsVisible: false
    function registerDialog(dialog) {
        dialogs = dialogs.filter(d => d !== dialog).concat([dialog])
        let entries = [], used = []
        function visit(item) {
            if (!item || !item.visible) return
            if (item instanceof T.AbstractButton || item instanceof T.TextField || item instanceof T.ComboBox || item instanceof T.SpinBox) {
                const label = item instanceof T.AbstractButton ? item.contentItem : item.Accessible.labelledBy
                const text = item instanceof T.AbstractButton ? item.text : label ? label.text : item.Accessible.name
                const key = Array.from((text || "").toUpperCase()).find(c => /^[A-IXK-W]$/.test(c) && !used.includes(c))
                if (key) { entries.push({item:item, label:label, key:key}); used.push(key) }
                return
            }
            for (const child of item.children || []) visit(child)
        }
        visit(dialog.contentItem); visit(dialog.footer)
        dialogTargets = entries
    }
    function unregisterDialog(dialog) {
        dialogs = dialogs.filter(d => d !== dialog)
        dialogTargets = []
        mnemonicsVisible = false
        if (dialogs.length) registerDialog(dialogs[dialogs.length-1])
    }
    function watch() {
        const next = hostWindow.activeFocusItem
        if (observed === next) return
        if (observed) observed.Keys.forwardTo = previousForwarding
        observed = next
        if (observed) {
            previousForwarding = Array.from(observed.Keys.forwardTo).filter(item => item !== hints)
            observed.Keys.forwardTo = [hints].concat(previousForwarding)
        }
    }
    Connections { target: hints.hostWindow; function onActiveFocusItemChanged() { hints.watch() } }
    Component.onCompleted: watch()
    Component.onDestruction: if (observed) observed.Keys.forwardTo = previousForwarding
    function popupContext() {
        for (let item = hostWindow.activeFocusItem; item; item = item.parent)
            if (item === hostWindow.Overlay.overlay) return true
        return false
    }
    function utilities() {
        const result = []
        function visit(item) {
            if (item.utilityKey !== undefined) { result.push({item:item, key:item.utilityKey}); return }
            for (const child of item.children || []) visit(child)
        }
        if (ribbon.currentPage) visit(ribbon.currentPage)
        return result
    }
    function commandKey(item) {
        const command = item.commandAction || item.action
        return command && command.keyHint ? command.keyHint : item.keyHint || ""
    }
    function pageTargets(entries) { return entries.map(item => ({item:item, key:commandKey(item)})).filter(t => t.key.length) }
    function showTabs() {
        level = 1; prefix = ""; group = null
        targets = tabBar.names.map((name,i) => ({item:tabBar.itemAt(i), key:tabKeys[name], tab:name})).concat(utilities())
    }
    function start() {
        if (popupContext()) return
        invoker = hostWindow.activeFocusItem
        explanation = ""
        showTabs()
    }
    function page() {
        level = 2; prefix = ""; group = null
        let result = []
        ribbon.groups().forEach((g, i) => {
            if (g.presentation === 2) result.push({item:g.collapseControl, key:"G" + (i+1), group:g})
            else result = result.concat(pageTargets(g.entries()))
        })
        targets = result
    }
    function stop(restore) {
        level = 0; targets = []; prefix = ""; explanation = ""
        if (restore && invoker && invoker.visible && invoker.enabled) invoker.forceActiveFocus(Qt.OtherFocusReason)
    }
    function menuOpened(openedMenu) {
        if (!level) return
        menu = openedMenu
        level = 3; prefix = ""
        let entries = []
        for (let i = 0; i < menu.count; ++i) {
            const item = menu.itemAt(i)
            if (item && item.visible) entries.push({item:item, key:commandKey(item) || "ABCDEFGHIKLMNOPQRSTUVWX"[i]})
        }
        targets = entries
    }
    function choose(target) {
        prefix = ""
        if (target.tab) { tabBar.selected(target.tab); Qt.callLater(page); return }
        if (target.group) { group = target.group; group.open(); level = 3; targets = pageTargets(group.entries()); return }
        const item = target.item
        if (!item.enabled) {
            explanation = item.effectiveTip || item.Accessible.description || item.tip || "This action is unavailable"
            return
        }
        const oldMenu = menu
        const oldLevel = level
        ribbon.activate(item)
        if (oldMenu) { oldMenu.close(); menu = null }
        // A command opening a RibbonMenu installs its next level synchronously.
        if (level === oldLevel && menu === null) stop(false)
    }
    Keys.onShortcutOverride: event => { if (level) event.accepted = true }
    Keys.onPressed: event => {
        if (event.key === Qt.Key_Alt && event.modifiers !== Qt.GroupSwitchModifier && !event.isAutoRepeat) {
            mnemonicsVisible = dialogs.length > 0
            altAlone = !popupContext(); event.accepted = false; return
        }
        if (event.key !== Qt.Key_Alt) altAlone = false
        if (!level && event.key === Qt.Key_F10 && !event.modifiers && !popupContext()) { start(); event.accepted = true; return }
        if (!level) {
            const target = dialogs.length && event.modifiers === Qt.AltModifier ? dialogTargets.find(t => t.key === event.text.toUpperCase()) : null
            if (target && target.item.enabled) { ribbon.activate(target.item); event.accepted = true }
            else event.accepted = false
            return
        }
        event.accepted = true
        if (event.key === Qt.Key_Escape) {
            if (menu) { menu.close(); menu = null; page() }
            else if (level === 3) { if (group) group.close(); page() }
            else if (level === 2) showTabs()
            else stop(true)
            return
        }
        const letter = event.text.toUpperCase()
        if (!/^[A-Z0-9]$/.test(letter) || event.modifiers & (Qt.ControlModifier | Qt.MetaModifier | Qt.GroupSwitchModifier)) return
        prefix += letter
        const matches = targets.filter(t => t.key.startsWith(prefix))
        const target = matches.find(t => t.key === prefix)
        if (target) choose(target)
        else if (!matches.length) prefix = ""
    }
    Keys.onReleased: event => {
        if (event.key === Qt.Key_Alt) {
            mnemonicsVisible = false
            if (altAlone) { if (level) stop(true); else start() }
            altAlone = false
        }
        event.accepted = level > 0
    }
    // Sibling overlay items never intercept pointer input.
    Repeater {
        model: hints.level ? hints.targets : []
        delegate: Label {
            required property var modelData
            readonly property point position: modelData.item.mapToItem(hints, 0, 0)
            x: position.x + Math.max(0, (modelData.item.width - width)/2)
            y: position.y + modelData.item.height - height / 2
            z: 100
            text: modelData.key
            visible: !hints.prefix.length || modelData.key.startsWith(hints.prefix)
            padding: 2
            color: palette.highlightedText
            font.bold: true
            background: Rectangle { color: palette.highlight; radius: 2 }
        }
    }
    Repeater {
        model: hints.mnemonicsVisible ? hints.dialogTargets.filter(t => t.label && t.label.text) : []
        delegate: Rectangle {
            required property var modelData
            readonly property var label: modelData.label
            readonly property point position: label.mapToItem(hints, 0, 0)
            readonly property int letterIndex: label.text.toUpperCase().indexOf(modelData.key)
            FontMetrics { id: labelMetrics; font: label.font }
            x: position.x + (label.horizontalAlignment === Text.AlignHCenter ? (label.width - labelMetrics.advanceWidth(label.text))/2 : 0) + labelMetrics.advanceWidth(label.text.slice(0,letterIndex))
            y: position.y + label.baselineOffset + 1
            width: labelMetrics.advanceWidth(modelData.key)
            height: 1
            color: label.color
            visible: letterIndex >= 0
        }
    }
    ToolTip.visible: explanation.length > 0
    ToolTip.text: explanation
}
