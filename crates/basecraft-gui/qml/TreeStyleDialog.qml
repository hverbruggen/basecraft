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

    title: "Tree style"
    modal: true
    padding: 16

    ListModel { id: sets }

    function openStyle() {
        const s = JSON.parse(controller.style_json())
        if (!s) return
        style = s.style
        sets.clear()
        // Shown sets first, in their order; then the hidden ones.
        const shown = style.support_shown.length ? style.support_shown : s.sets.map(x => x.name)
        for (const name of shown) {
            const x = s.sets.find(x => x.name === name)
            if (x) sets.append({ name: x.name, source: x.source, shown: true })
        }
        for (const x of s.sets)
            if (!shown.includes(x.name)) sets.append({ name: x.name, source: x.source, shown: false })
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

    function save() {
        const shown = []
        for (let i = 0; i < sets.count; i++) if (sets.get(i).shown) shown.push(sets.get(i).name)
        const s = Object.assign({}, style, {
            support_shown: shown,
            support_min: minimum.value,
            symbols: symbols.checked && symbolSet.currentText.length
                ? { set: symbolSet.currentText, high: high.value, low: low.value } : null,
            tip_font: tipFont.value,
            support_font: supportFont.value,
            branch_width: branchWidth.value,
            vertical_bar_labels: verticalLabels.checked,
            scale_bar: scaleBar.checked,
        })
        const err = controller.set_style(JSON.stringify(s))
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
            RowLayout {
                required property int index
                required property string name
                required property string source
                required property bool shown
                CheckBox {
                    text: name + (source.length ? "  (" + source + ")" : "")
                    checked: shown
                    onToggled: sets.setProperty(index, "shown", checked)
                    Layout.fillWidth: true
                }
                AppIconButton { iconName: "up"; description: "Move colour set up"; enabled: index > 0; onClicked: sets.move(index, index - 1, 1) }
                AppIconButton { iconName: "down"; description: "Move colour set down"; enabled: index < sets.count - 1; onClicked: sets.move(index, index + 1, 1) }
                ToolButton {
                    text: "Remove"
                    onClicked: { controller.remove_support(name); dialog.openStyle() }
                }
            }
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
