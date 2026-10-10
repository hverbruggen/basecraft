import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Add or edit one feature: type, location in GenBank syntax (1-based, with
// complement() and join()), and qualifiers. Saving is one operation.
Dialog {
    id: dialog
    property var controller
    property int featureId: -1

    title: featureId < 0 ? "Add feature" : "Edit feature"
    modal: true
    anchors.centerIn: Overlay.overlay
    width: Math.min(640, parent ? parent.width - 40 : 640)
    height: Math.min(560, parent ? parent.height - 40 : 560)
    padding: 16

    function load(json) {
        const f = JSON.parse(json)
        if (!f) return false
        featureId = f.id
        kind.editText = f.kind
        const i = kind.find(f.kind)
        kind.currentIndex = i
        if (i < 0) kind.editText = f.kind
        location.text = f.location
        quals.clear()
        for (const q of f.qualifiers) {
            if (q.key.startsWith("gff:")) hidden.push(q); else quals.append({ key: q.key, value: q.value, flag: q.flag })
        }
        error.text = ""
        return true
    }
    property var hidden: []

    function openFeature(id) { hidden = []; if (load(controller.feature_json(id))) open() }
    function openSelected() {
        const id = controller.selected_feature()
        if (id >= 0) openFeature(id); else controller.message = "Click a feature first, or select residues and use Add feature"
    }
    function openNew() { hidden = []; load(controller.new_feature_json()); open(); location.forceActiveFocus() }

    function save() {
        const qualifiers = hidden.slice()
        for (let i = 0; i < quals.count; i++) {
            const q = quals.get(i)
            qualifiers.push({ key: q.key, value: q.value, flag: q.flag && q.value === "" })
        }
        const obj = { kind: kind.editText, location: location.text, qualifiers: qualifiers }
        const err = controller.save_feature(featureId, JSON.stringify(obj))
        if (err.length > 0) error.text = err; else close()
    }

    ListModel { id: quals }

    contentItem: ColumnLayout {
        spacing: 10
        GridLayout {
            columns: 2
            columnSpacing: 12
            rowSpacing: 8
            Layout.fillWidth: true
            Label { text: "Type" }
            ComboBox {
                id: kind
                editable: true
                Layout.fillWidth: true
                model: JSON.parse(dialog.controller ? dialog.controller.feature_kinds() : "[]")
            }
            Label { text: "Location" }
            TextField {
                id: location
                Layout.fillWidth: true
                font.family: "monospace"
                placeholderText: "e.g. 120..560, complement(join(10..80,200..300))"
                onAccepted: dialog.save()
            }
        }
        Label { text: "Qualifiers"; font.bold: true }
        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: quals
            spacing: 4
            delegate: RowLayout {
                width: list.width
                spacing: 6
                required property int index
                required property string key
                required property string value
                TextField {
                    Layout.preferredWidth: 150
                    text: key
                    placeholderText: "key"
                    onTextEdited: quals.setProperty(index, "key", text)
                }
                TextField {
                    Layout.fillWidth: true
                    text: value
                    placeholderText: "value"
                    onTextEdited: quals.setProperty(index, "value", text)
                    onAccepted: dialog.save()
                }
                ToolButton { text: "×"; onClicked: quals.remove(index) }
            }
        }
        RowLayout {
            Button { text: "Add qualifier"; onClicked: quals.append({ key: "", value: "", flag: false }) }
            Item { Layout.fillWidth: true }
            Button {
                text: "Delete feature"
                visible: dialog.featureId >= 0
                onClicked: { dialog.controller.delete_feature(dialog.featureId); dialog.close() }
            }
            Button { text: "Cancel"; onClicked: dialog.close() }
            Button { text: "Save"; highlighted: true; onClicked: dialog.save() }
        }
        Label {
            id: error
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: dialog.controller ? dialog.controller.accent : "red"
            visible: text.length > 0
        }
    }
}
