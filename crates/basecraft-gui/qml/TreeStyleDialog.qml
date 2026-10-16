import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// How a tree is drawn: which support values show and in what order,
// circles at nodes, sizes. Saving is one operation.
AppContentDialog {
    id: dialog
    initialFocusItem: minimum
    property var controller
    property var style: ({})
    property var removedSets: []

    title: "Tree style"
    modal: true
    padding: 16

    ListModel { id: sets }
    FontMetrics { id: styleMetrics; font: dialog.font }

    function openStyle() {
        const s = JSON.parse(controller.style_json())
        if (!s) return
        style = s.style
        sets.clear()
        removedSets = []
        // Shown sets first, in their order; then the hidden ones.
        const shown = style.support_shown.length ? style.support_shown : s.sets.map(x => x.name)
        for (const name of shown) {
            const x = s.sets.find(x => x.name === name)
            if (x) sets.append({ name: x.name, source: x.source, shown: true, decimals: x.decimals, autoDefault: x.automatic_decimals, scale: scaleIndex(x.name), autoDecimals: !Object.prototype.hasOwnProperty.call(style.support_decimals || {}, x.name) })
        }
        for (const x of s.sets)
            if (!shown.includes(x.name)) sets.append({ name: x.name, source: x.source, shown: false, decimals: x.decimals, autoDefault: x.automatic_decimals, scale: scaleIndex(x.name), autoDecimals: !Object.prototype.hasOwnProperty.call(style.support_decimals || {}, x.name) })
        minimum.value = style.support_min
        symbols.checked = style.symbols !== null
        symbolSet.model = s.sets.map(x => x.name)
        symbolSet.currentIndex = style.symbols ? Math.max(0, symbolSet.model.indexOf(style.symbols.set)) : 0
        high.value = style.symbols ? style.symbols.high : 90
        low.value = style.symbols ? style.symbols.low : 70
        tipFont.value = style.tip_font
        supportFont.value = style.support_font
        branchWidth.value = style.branch_width
        verticalLabels.checked = style.vertical_bar_labels
        scaleBar.checked = style.scale_bar
        error.text = ""
        open()
    }

    function scaleIndex(name) {
        const overrides = style.support_proportions || {}
        return Object.prototype.hasOwnProperty.call(overrides, name) ? (overrides[name] ? 2 : 1) : 0
    }

    function save() {
        const shown = []
        for (let i = 0; i < sets.count; i++) if (sets.get(i).shown) shown.push(sets.get(i).name)
        const decimals = Object.assign({}, style.support_decimals || {})
        const proportions = Object.assign({}, style.support_proportions || {})
        for (let i = 0; i < sets.count; i++) {
            const set = sets.get(i)
            if (set.autoDecimals) delete decimals[set.name]
            else decimals[set.name] = set.decimals
            if (set.scale === 0) delete proportions[set.name]
            else proportions[set.name] = set.scale === 2
        }
        for (const name of removedSets) {
            delete decimals[name]
            delete proportions[name]
        }
        const s = Object.assign({}, style, {
            support_decimals: decimals,
            support_proportions: proportions,
            support_shown: shown,
            support_min: minimum.value,
            symbols: symbols.checked && symbolSet.currentText.length && !removedSets.includes(symbolSet.currentText)
                ? { set: symbolSet.currentText, high: high.value, low: low.value } : null,
            tip_font: tipFont.value,
            support_font: supportFont.value,
            branch_width: branchWidth.value,
            vertical_bar_labels: verticalLabels.checked,
            scale_bar: scaleBar.checked,
        })
        const err = controller.set_style_with_removals(JSON.stringify(s), JSON.stringify(removedSets))
        if (err.length > 0) error.text = err
        else close()
    }

    // The dialog takes its size from its contents.
    body: ColumnLayout {
        spacing: 8

        Label { text: "Support values, shown in this order and joined with /"; font.bold: true }
        Label { text: "This tree has no support values yet"; visible: sets.count === 0; opacity: 0.7 }
        Repeater {
            model: sets
            ColumnLayout {
                id: supportRow
                required property int index
                required property string name
                required property string source
                required property bool shown
                required property int decimals
                required property int autoDefault
                required property int scale
                required property bool autoDecimals
                Layout.fillWidth: true
                RowLayout {
                    Layout.fillWidth: true
                    CheckBox {
                        text: supportRow.name + (supportRow.source.length ? "  (" + supportRow.source + ")" : "")
                        checked: supportRow.shown
                        onToggled: sets.setProperty(supportRow.index, "shown", checked)
                        Layout.fillWidth: true
                    }
                    AppIconButton { iconName: "up"; description: "Move support set up"; enabled: supportRow.index > 0; onClicked: sets.move(supportRow.index, supportRow.index - 1, 1) }
                    AppIconButton { iconName: "down"; description: "Move support set down"; enabled: supportRow.index < sets.count - 1; onClicked: sets.move(supportRow.index, supportRow.index + 1, 1) }
                    ToolButton {
                        text: "Remove"
                        objectName: "removeSupport" + supportRow.index
                        onClicked: {
                            dialog.removedSets = dialog.removedSets.concat([supportRow.name])
                            sets.remove(supportRow.index)
                        }
                    }
                }
                GridLayout {
                    id: precisionSettings
                    columns: width < styleMetrics.averageCharacterWidth * 50 ? 2 : 3
                    Layout.fillWidth: true
                    CheckBox {
                        objectName: "autoDecimals" + supportRow.index
                        text: "Auto decimals"
                        checked: supportRow.autoDecimals
                        Accessible.name: supportRow.name + " automatic decimal places"
                        onToggled: {
                            sets.setProperty(supportRow.index, "decimals", precision.value)
                            sets.setProperty(supportRow.index, "autoDecimals", checked)
                        }
                    }
                    SpinBox {
                        id: precision
                        objectName: "supportDecimals" + supportRow.index
                        Accessible.name: supportRow.name + " decimal places"
                        from: 0; to: 10; editable: true
                        value: supportRow.autoDecimals
                            ? (supportRow.scale === 0 ? supportRow.autoDefault : supportRow.scale === 2 ? 2 : 0)
                            : supportRow.decimals
                        enabled: !supportRow.autoDecimals
                        onValueModified: sets.setProperty(supportRow.index, "decimals", value)
                    }
                    AppComboBox {
                        objectName: "supportScale" + supportRow.index
                        Layout.columnSpan: precisionSettings.columns === 2 ? 2 : 1
                        Accessible.name: supportRow.name + " support scale"
                        model: ["Auto scale", "Percent (0–100)", "Proportion (0–1)"]
                        currentIndex: supportRow.scale
                        onActivated: sets.setProperty(supportRow.index, "scale", currentIndex)
                        Layout.fillWidth: true
                    }
                }
            }
        }
        Label {
            text: "Auto: bootstrap uses 0 decimals; probabilities use 2. Unspecified values all in 0–1 use probability formatting. Scale changes display thresholds only."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
            opacity: 0.7
        }
        RowLayout {
            Label { id: minimumLabel; text: "Hide values below" }
            SpinBox { id: minimum; objectName: "minimum"; Accessible.labelledBy: minimumLabel; Accessible.name: minimumLabel.text + " (%)"; from: 0; to: 100; editable: true }
            Label { text: "% (posteriors count as %)"; opacity: 0.7 }
        }

        RowLayout {
            CheckBox { id: symbols; text: "Circles at nodes for" }
            AppComboBox { id: symbolSet; objectName: "symbolSet"; Accessible.labelledBy: symbols; Accessible.name: symbols.text; enabled: symbols.checked; Layout.preferredWidth: 150 }
        }
        RowLayout {
            enabled: symbols.checked
            Label { id: highLabel; text: "Dark at"; leftPadding: 28 }
            SpinBox { id: high; objectName: "high"; Accessible.labelledBy: highLabel; Accessible.name: highLabel.text; from: 0; to: 100; editable: true }
            Label { id: lowLabel; text: "grey at" }
            SpinBox { id: low; objectName: "low"; Accessible.labelledBy: lowLabel; Accessible.name: lowLabel.text; from: 0; to: 100; editable: true }
            Label { text: "white below" }
        }

        Label { text: "Sizes"; font.bold: true; topPadding: 6 }
        GridLayout {
            columns: 4
            Label { id: tipFontLabel; text: "Tip names" }
            SpinBox { id: tipFont; objectName: "tipFont"; Accessible.labelledBy: tipFontLabel; Accessible.name: tipFontLabel.text; from: 6; to: 40; editable: true }
            Label { id: supportFontLabel; text: "Support values" }
            SpinBox { id: supportFont; objectName: "supportFont"; Accessible.labelledBy: supportFontLabel; Accessible.name: supportFontLabel.text; from: 6; to: 30; editable: true }
            Label { id: branchWidthLabel; text: "Branches" }
            SpinBox { id: branchWidth; objectName: "branchWidth"; Accessible.labelledBy: branchWidthLabel; Accessible.name: branchWidthLabel.text; from: 1; to: 10; editable: true }
        }
        CheckBox { id: verticalLabels; text: "Clade names beside bars run top to bottom" }
        CheckBox { id: scaleBar; text: "Scale bar" }

        AppErrorLabel { id: error; visible: text.length > 0; color: palette.highlight; wrapMode: Text.Wrap; Layout.fillWidth: true }

        RowLayout {
            Item { Layout.fillWidth: true }
            AppButton { text: "Cancel"; onClicked: dialog.close() }
            AppButton { text: "Save"; highlighted: true; onClicked: dialog.save() }
        }
    }
}
