import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

// Annotate the selected clade: a name on its stem branch, a bar along its
// tips, a field of colour behind it. Saving is one operation.
Dialog {
    id: dialog
    property var controller
    property var annotation: ({})

    title: annotation.exists ? "Edit clade annotation" : "Annotate clade"
    modal: true
    anchors.centerIn: Overlay.overlay
    width: Math.min(460, parent ? parent.width - 40 : 460)
    padding: 16

    // Colours offered as swatches; any other can be typed or picked.
    readonly property var swatches: ["#000000", "#808080", "#c0392b", "#e67e22", "#d4ac0d", "#27ae60", "#2980b9", "#8e44ad"]

    function openSelected() {
        const a = JSON.parse(controller.annotation_json())
        if (!a) return
        annotation = a
        label.text = a.label
        onBranch.checked = a.on_branch
        bar.checked = a.bar
        barWidth.value = a.bar_width
        field.checked = a.field
        colour.text = a.colour
        left.value = a.opacity_left
        right.value = a.opacity_right
        error.text = ""
        open()
    }

    function save() {
        const a = Object.assign({}, annotation, {
            label: label.text,
            on_branch: onBranch.checked,
            bar: bar.checked,
            bar_width: barWidth.value,
            field: field.checked,
            colour: colour.text.trim(),
            opacity_left: Math.round(left.value),
            opacity_right: Math.round(right.value),
        })
        const err = controller.set_annotation(JSON.stringify(a))
        if (err.length > 0) error.text = err
        else close()
    }

    onOpened: { label.forceActiveFocus(); label.selectAll() }

    ColorDialog {
        id: picker
        selectedColor: colour.text
        onAccepted: colour.text = selectedColor.toString()
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 10

        Label { text: (annotation.tips || 0) + " tips"; opacity: 0.7 }

        RowLayout {
            Label { text: "Name"; Layout.preferredWidth: 70 }
            TextField {
                id: label
                Layout.fillWidth: true
                placeholderText: "e.g. Ventalia"
                Keys.onReturnPressed: dialog.save()
                Keys.onEnterPressed: dialog.save()
            }
        }

        CheckBox { id: onBranch; text: "Name on the stem branch" }
        RowLayout {
            CheckBox { id: bar; text: "Bar along the tips, named beside it" }
            Item { Layout.fillWidth: true }
            Label { text: "Thickness"; enabled: bar.checked }
            SpinBox { id: barWidth; from: 1; to: 30; enabled: bar.checked; editable: true }
        }
        CheckBox { id: field; text: "Coloured field behind the clade" }

        RowLayout {
            Label { text: "Colour"; Layout.preferredWidth: 70 }
            Repeater {
                model: dialog.swatches
                Rectangle {
                    required property string modelData
                    width: 20; height: 20; radius: 3
                    color: modelData
                    border.width: colour.text.toLowerCase() === modelData ? 2 : 1
                    border.color: colour.text.toLowerCase() === modelData ? palette.highlight : Qt.alpha(palette.text, 0.3)
                    MouseArea { anchors.fill: parent; onClicked: colour.text = parent.modelData }
                }
            }
            TextField { id: colour; Layout.preferredWidth: 90 }
            Button { text: "Pick…"; onClicked: picker.open() }
        }

        GridLayout {
            columns: 3
            enabled: field.checked
            Label { text: "Field opacity at left" }
            Slider { id: left; from: 0; to: 100; stepSize: 1; Layout.fillWidth: true }
            Label { text: Math.round(left.value) + " %"; Layout.preferredWidth: 44 }
            Label { text: "Field opacity at right" }
            Slider { id: right; from: 0; to: 100; stepSize: 1; Layout.fillWidth: true }
            Label { text: Math.round(right.value) + " %"; Layout.preferredWidth: 44 }
        }
        // What the field will look like.
        Rectangle {
            Layout.fillWidth: true
            height: 18
            visible: field.checked
            border.color: Qt.alpha(palette.text, 0.2)
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Qt.alpha(colour.text, left.value / 100) }
                GradientStop { position: 1; color: Qt.alpha(colour.text, right.value / 100) }
            }
        }

        Label { id: error; visible: text.length > 0; color: palette.highlight; wrapMode: Text.Wrap; Layout.fillWidth: true }

        RowLayout {
            Button {
                text: "Remove"
                visible: annotation.exists === true
                onClicked: { controller.remove_annotation(annotation.id); dialog.close() }
            }
            Item { Layout.fillWidth: true }
            Button { text: "Cancel"; onClicked: dialog.close() }
            Button { text: "Save"; highlighted: true; onClicked: dialog.save() }
        }
    }
}
