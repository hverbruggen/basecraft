import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Annotate the selected clade: a name on its stem branch, a bar along its
// tips, a field of colour behind it. Saving is one operation.
AppContentDialog {
    id: dialog
    initialFocusItem: label
    property var controller
    property var annotation: ({})
    property var editController: null
    property string editRevision: ""

    title: annotation.exists ? "Edit clade annotation" : "Annotate clade"
    padding: 16

    function openSelected() {
        if (!controller) return
        openCaptured(controller, JSON.parse(controller.annotation_json()), controller.record_revision())
    }

    // A context menu captures this before opening: selection or tabs may change
    // while the menu/dialog is shown, but the annotation keeps its original clade.
    function openCaptured(target, a, revision, nameOnBranch = false) {
        if (!a) return
        if (a.error) {
            target.message_kind = "error"
            target.message = a.error
            return
        }
        editController = target
        editRevision = revision
        annotation = a
        label.text = a.label
        onBranch.checked = a.on_branch || (nameOnBranch && !a.exists)
        bar.checked = a.bar && !(nameOnBranch && !a.exists)
        barWidth.value = a.bar_width
        field.checked = a.field
        colour.text = a.colour
        left.value = a.opacity_left
        right.value = a.opacity_right
        error.text = ""
        open()
    }

    function validTarget() {
        if (!editController || !editRevision.length || editController.record_revision() !== editRevision) {
            error.text = "The tree changed or closed. Close this editor and open the annotation again."
            return false
        }
        return true
    }

    function save() {
        if (!validTarget()) return
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
        const err = editController.set_annotation(JSON.stringify(a))
        if (err.length > 0) error.text = err
        else close()
    }

    onOpened: { label.forceActiveFocus(); label.selectAll() }

    // The dialog takes its size from its contents.
    body: ColumnLayout {
        spacing: 10

        Label { text: (annotation.tip_count || 0) + " tips"; opacity: 0.7 }

        RowLayout {
            Label { id: cladeNameLabel; text: "Name"; Layout.preferredWidth: 70 }
            TextField {
                id: label; objectName: "label"; Accessible.labelledBy: cladeNameLabel; Accessible.name: cladeNameLabel.text
                Layout.fillWidth: true
                Layout.minimumWidth: 300
                placeholderText: "e.g. Ventalia"
                Keys.onReturnPressed: dialog.save()
                Keys.onEnterPressed: dialog.save()
            }
        }

        AppCheckBox { id: onBranch; objectName: "onBranch"; text: "Name on the stem branch" }
        RowLayout {
            AppCheckBox { id: bar; objectName: "bar"; text: "Bar along the tips, named beside it" }
            Item { Layout.fillWidth: true }
            Label { id: barWidthLabel; text: "Thickness"; enabled: bar.checked }
            AppSpinBox { id: barWidth; objectName: "barWidth"; Accessible.labelledBy: barWidthLabel; Accessible.name: barWidthLabel.text; from: 1; to: 30; enabled: bar.checked; editable: true }
        }
        AppCheckBox { id: field; text: "Coloured field behind the clade" }

        ColorPicker { id: colour }

        GridLayout {
            columns: 3
            enabled: field.checked
            Label { id: leftOpacityLabel; text: "Field opacity at left" }
            Slider { id: left; objectName: "left"; Accessible.labelledBy: leftOpacityLabel; Accessible.name: leftOpacityLabel.text + " (%)"; from: 0; to: 100; stepSize: 1; Layout.fillWidth: true }
            Label { text: Math.round(left.value) + " %"; Layout.preferredWidth: 44 }
            Label { id: rightOpacityLabel; text: "Field opacity at right" }
            Slider { id: right; objectName: "right"; Accessible.labelledBy: rightOpacityLabel; Accessible.name: rightOpacityLabel.text + " (%)"; from: 0; to: 100; stepSize: 1; Layout.fillWidth: true }
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

        AppErrorLabel { id: error; objectName: "annotationError"; visible: text.length > 0; color: palette.highlight; wrapMode: Text.Wrap; Layout.fillWidth: true }

        RowLayout {
            AppButton {
                text: "Remove"
                visible: annotation.exists === true
                onClicked: {
                    if (!dialog.validTarget()) return
                    editController.remove_annotation(annotation.id)
                    dialog.close()
                }
            }
            Item { Layout.fillWidth: true }
            AppButton { text: "Cancel"; onClicked: dialog.reject() }
            AppButton { text: "Save"; highlighted: true; onClicked: dialog.save() }
        }
    }
}
