import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Dialogs
import QtQuick.Layouts

// One colour, from swatches, a wheel, a #rrggbb field or the system colour
// dialog. Every dialog that asks for a colour uses this.
ColumnLayout {
    id: picker
    // The colour as typed; `valid` when it is #rrggbb.
    property alias text: hex.text
    property string label: "Colour"
    property var swatches: ["#000000", "#808080", "#c0392b", "#e67e22", "#d4ac0d", "#27ae60", "#2980b9", "#8e44ad"]
    readonly property bool valid: isHex(hex.text)
    function isHex(value) { return /^#[0-9a-fA-F]{6}$/.test(value.trim()) }
    // A colour chosen with any of the controls, as lower-case #rrggbb.
    signal edited(string value)
    spacing: 8

    function choose(value) {
        hex.text = value.toLowerCase()
        edited(hex.text)
    }

    ColorWheel {
        id: wheel
        objectName: "colorWheel"
        onEdited: value => picker.choose(value.toString())
    }
    RowLayout {
        spacing: 4
        Repeater {
            model: picker.swatches
            Rectangle {
                required property string modelData
                width: 20; height: 20; radius: 3
                color: modelData
                readonly property bool current: hex.text.toLowerCase() === modelData
                border.width: current ? 2 : 1
                border.color: current ? palette.highlight : Qt.alpha(palette.text, 0.3)
                MouseArea { anchors.fill: parent; onClicked: picker.choose(parent.modelData) }
            }
        }
    }
    RowLayout {
        Label { id: hexLabel; text: picker.label }
        TextField {
            id: hex
            objectName: "colorHex"
            Layout.preferredWidth: 100
            maximumLength: 7
            font.family: "monospace"
            Accessible.labelledBy: hexLabel
            Accessible.name: hexLabel.text
            // The wheel follows the field, however the text was set.
            // Checked here, not with `valid`, which may not have caught up yet.
            onTextChanged: if (picker.isHex(text)) wheel.color = text.trim()
            onTextEdited: if (picker.isHex(text)) picker.edited(text.trim().toLowerCase())
        }
        Rectangle {
            width: 28; height: 20; radius: 3
            color: picker.valid ? hex.text.trim() : "transparent"
            border.color: palette.mid
        }
        AppButton { text: "Pick…"; onClicked: system.open() }
    }
    Label {
        visible: !picker.valid
        text: "Use #rrggbb"
        color: palette.highlight
    }
    ColorDialog {
        id: system
        selectedColor: picker.valid ? hex.text.trim() : "#808080"
        onAccepted: picker.choose(selectedColor.toString())
    }
}
