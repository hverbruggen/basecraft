import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// How a tree is drawn, in two parts: "support" (which support values show
// and how, circles at nodes) and "extra" (tip names, branches, labels, scale
// bar). Saving is one operation.
AppContentDialog {
    id: dialog
    initialFocusItem: section === "support" ? placement : branchWidth
    property var controller
    property var style: ({})
    property var removedSets: []
    property string section: "extra"

    title: section === "support" ? "Support style" : "Tree style"
    padding: 16

    ListModel { id: sets }
    FontMetrics { id: styleMetrics; font: dialog.font }

    function openStyle(part) {
        section = part || "extra"
        const s = JSON.parse(controller.style_json())
        if (!s) return
        style = s.style
        sets.clear()
        removedSets = []
        // Shown sets first, in their order; then the hidden ones.
        const shown = style.support_shown.length ? style.support_shown : s.sets.map(x => x.name)
        for (const name of shown) {
            const x = s.sets.find(x => x.name === name)
            if (x) sets.append({ name: x.name, source: x.source, shown: true, decimals: x.decimals, autoDefault: x.automatic_decimals, scale: scaleIndex(x), minimum: (style.support_minimums || {})[x.name] === undefined ? style.support_min : style.support_minimums[x.name], autoDecimals: !Object.prototype.hasOwnProperty.call(style.support_decimals || {}, x.name) })
        }
        for (const x of s.sets)
            if (!shown.includes(x.name)) sets.append({ name: x.name, source: x.source, shown: false, decimals: x.decimals, autoDefault: x.automatic_decimals, scale: scaleIndex(x), minimum: (style.support_minimums || {})[x.name] === undefined ? style.support_min : style.support_minimums[x.name], autoDecimals: !Object.prototype.hasOwnProperty.call(style.support_decimals || {}, x.name) })
        placement.currentIndex = ["natural", "above", "below"].indexOf(style.support_placement || "natural")
        legend.checked = style.support_legend !== false
        symbols.checked = style.symbols !== null
        symbolSet.model = s.sets.map(x => x.name)
        symbolSet.currentIndex = style.symbols ? Math.max(0, symbolSet.model.indexOf(style.symbols.set)) : 0
        high.value = style.symbols ? style.symbols.high : 90
        low.value = style.symbols ? style.symbols.low : 70
        branchWidth.value = style.branch_width
        verticalLabels.checked = style.vertical_bar_labels
        scaleBar.checked = style.scale_bar
        error.text = ""
        open()
    }

    function scaleIndex(set) {
        const overrides = style.support_fraction || {}
        return (Object.prototype.hasOwnProperty.call(overrides, set.name) ? overrides[set.name] : set.proportions) ? 1 : 0
    }

    function save() {
        const shown = []
        for (let i = 0; i < sets.count; i++) if (sets.get(i).shown) shown.push(sets.get(i).name)
        const decimals = Object.assign({}, style.support_decimals || {})
        const proportions = Object.assign({}, style.support_proportions || {})
        const fractions = Object.assign({}, style.support_fraction || {})
        const minimums = Object.assign({}, style.support_minimums || {})
        for (let i = 0; i < sets.count; i++) {
            const set = sets.get(i)
            if (set.autoDecimals) delete decimals[set.name]
            else decimals[set.name] = set.decimals
            fractions[set.name] = set.scale === 1
            minimums[set.name] = set.minimum
        }
        for (const name of removedSets) {
            delete decimals[name]
            delete proportions[name]
            delete fractions[name]
            delete minimums[name]
        }
        const s = Object.assign({}, style, {
            support_decimals: decimals,
            support_proportions: proportions,
            support_fraction: fractions,
            support_minimums: minimums,
            support_placement: ["natural", "above", "below"][placement.currentIndex],
            support_legend: legend.checked,
            support_shown: shown,
            symbols: symbols.checked && symbolSet.currentText.length && !removedSets.includes(symbolSet.currentText)
                ? { set: symbolSet.currentText, high: high.value, low: low.value } : null,
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

        ColumnLayout {
        visible: dialog.section === "support"
        spacing: 8
        Layout.fillWidth: true
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
                required property int minimum
                required property int scale
                required property bool autoDecimals
                Layout.fillWidth: true
                RowLayout {
                    Layout.fillWidth: true
                    AppCheckBox {
                        text: supportRow.name + (supportRow.source.length ? "  (" + supportRow.source + ")" : "")
                        font.bold: true
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
                    columns: width < styleMetrics.averageCharacterWidth * 60 ? 2 : 4
                    Layout.leftMargin: 24
                    Layout.fillWidth: true
                    AppCheckBox {
                        objectName: "autoDecimals" + supportRow.index
                        text: "Auto decimals"
                        checked: supportRow.autoDecimals
                        Accessible.name: supportRow.name + " automatic decimal places"
                        onToggled: {
                            sets.setProperty(supportRow.index, "decimals", precision.value)
                            sets.setProperty(supportRow.index, "autoDecimals", checked)
                        }
                    }
                    AppSpinBox {
                        id: precision
                        objectName: "supportDecimals" + supportRow.index
                        Accessible.name: supportRow.name + " decimal places"
                        from: 0; to: 10; editable: true
                        value: supportRow.autoDecimals
                            ? (supportRow.scale === 1 ? 2 : 0)
                            : supportRow.decimals
                        enabled: !supportRow.autoDecimals
                        onValueModified: sets.setProperty(supportRow.index, "decimals", value)
                    }
                    Label { text: "Scale" }
                    AppComboBox {
                        objectName: "supportScale" + supportRow.index
                        Accessible.name: supportRow.name + " support scale"
                        model: ["Percentage", "Fraction"]
                        currentIndex: supportRow.scale
                        onActivated: sets.setProperty(supportRow.index, "scale", currentIndex)
                        Layout.fillWidth: true
                    }
                }
                RowLayout {
                    Layout.leftMargin: 24
                    Label { id: minimumLabel; text: "Hide below (%)" }
                    AppSpinBox {
                        objectName: "supportMinimum" + supportRow.index
                        Accessible.labelledBy: minimumLabel
                        Accessible.name: supportRow.name + " hide below (percent)"
                        from: 0; to: 100; editable: true
                        value: supportRow.minimum
                        onValueModified: sets.setProperty(supportRow.index, "minimum", value)
                    }
                }
            }
        }
        RowLayout {
            Label { id: placementLabel; text: "Position" }
            AppComboBox { id: placement; objectName: "supportPlacement"; Accessible.name: "Support position"; model: ["Natural", "Above", "Below"] }
        }
        AppCheckBox { id: legend; objectName: "supportLegend"; text: "Support legend (multiple shown types)" }
        Label { text: "Circles at nodes"; font.bold: true; Layout.topMargin: 12 }
        RowLayout {
            AppCheckBox { id: symbols; text: "Circles at nodes for" }
            AppComboBox { id: symbolSet; objectName: "symbolSet"; Accessible.labelledBy: symbols; Accessible.name: symbols.text; enabled: symbols.checked; Layout.preferredWidth: 150 }
        }
        RowLayout {
            enabled: symbols.checked
            Label { id: highLabel; text: "Dark at"; leftPadding: 28 }
            AppSpinBox { id: high; objectName: "high"; Accessible.labelledBy: highLabel; Accessible.name: highLabel.text; from: 0; to: 100; editable: true }
            Label { id: lowLabel; text: "grey at" }
            AppSpinBox { id: low; objectName: "low"; Accessible.labelledBy: lowLabel; Accessible.name: lowLabel.text; from: 0; to: 100; editable: true }
            Label { text: "white below" }
        }
        }

        ColumnLayout {
        visible: dialog.section === "extra"
        spacing: 8
        GridLayout {
            columns: 2
            Label { id: branchWidthLabel; text: "Branch width (px)" }
            AppSpinBox { id: branchWidth; objectName: "branchWidth"; Accessible.labelledBy: branchWidthLabel; Accessible.name: branchWidthLabel.text; from: 1; to: 10; editable: true }
        }
        AppCheckBox { id: verticalLabels; text: "Clade names beside bars run top to bottom" }
        AppCheckBox { id: scaleBar; text: "Scale bar" }
        }

        AppErrorLabel { id: error; visible: text.length > 0; color: palette.highlight; wrapMode: Text.Wrap; Layout.fillWidth: true }

        RowLayout {
            Item { Layout.fillWidth: true }
            AppButton { text: "Cancel"; onClicked: dialog.reject() }
            AppButton { text: "Save"; highlighted: true; onClicked: dialog.save() }
        }
    }
}
