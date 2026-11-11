import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Add or edit one annotation (a GenBank/GFF feature): type, location in GenBank syntax (1-based, with
// complement() and join()), and qualifiers. Saving is one operation.
AppContentDialog {
    id: dialog
    initialFocusItem: location
    property var controller
    property int featureId: -1

    title: featureId < 0 ? "Add annotation" : "Edit annotation"
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
        refreshFromLocation()
        quals.clear()
        for (const q of f.qualifiers) {
            if (q.key.startsWith("gff:")) hidden.push(q); else quals.append({ key: q.key, value: q.value, flag: q.flag })
        }
        error.text = ""
        return true
    }
    property var hidden: []
    readonly property var strands: ["forward", "reverse", "none"]

    // The location text is the source of truth; the direction and
    // completeness controls read it and rewrite it.
    function refreshFromLocation() {
        const info = JSON.parse(controller.location_info(location.text))
        if (!info.valid) {
            locationInfo.text = "Can't read this location yet"
            return
        }
        direction.currentIndex = strands.indexOf(info.strand)
        partial5.checked = info.partial5
        partial3.checked = info.partial3
        locationInfo.text = info.length + " bp" + (info.parts > 1 ? " in " + info.parts + " parts" : "")
            + (info.strand === "mixed" ? " · mixed directions" : "")
    }
    function applyControls() {
        const strand = direction.currentIndex >= 0 ? strands[direction.currentIndex] : "mixed"
        const text = controller.adjust_location(location.text, strand, partial5.checked, partial3.checked)
        if (text.length > 0) location.text = text
        refreshFromLocation()
    }

    function openFeature(id) { hidden = []; if (load(controller.feature_json(id))) open() }
    function openSelected() {
        const id = controller.selected_feature()
        if (id >= 0) openFeature(id); else { controller.message_kind = ""; controller.message = "Click an annotation first, or select bases and use Add annotation" }
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

    // The value field of qualifier row `i`, for scripted tests.
    function valueField(i) {
        const row = list.itemAtIndex(i)
        return row ? row.children[1] : null
    }

    body: ColumnLayout {
        spacing: 10
        GridLayout {
            columns: width < 460 ? 1 : 2
            columnSpacing: 12
            rowSpacing: 8
            Layout.fillWidth: true
            Label { id: kindLabel; text: "Type" }
            AppComboBox {
                id: kind; objectName: "kind"; Accessible.labelledBy: kindLabel; Accessible.name: kindLabel.text
                editable: true
                Layout.fillWidth: true
                model: JSON.parse(dialog.controller ? dialog.controller.feature_kinds() : "[]")
            }
            Label { id: locationLabel; text: "Location" }
            TextField {
                id: location; objectName: "location"; Accessible.labelledBy: locationLabel; Accessible.name: locationLabel.text
                Layout.fillWidth: true
                font.family: "monospace"
                placeholderText: "e.g. 120..560, complement(join(10..80,200..300))"
                onAccepted: dialog.save()
                onTextEdited: dialog.refreshFromLocation()
            }
            Label { id: directionLabel; text: "Direction" }
            RowLayout {
                spacing: 12
                AppComboBox {
                    id: direction; objectName: "direction"; Accessible.labelledBy: directionLabel; Accessible.name: directionLabel.text
                    model: ["Forward  →", "Reverse  ←", "None"]
                    displayText: currentIndex < 0 ? "Mixed" : currentText
                    onActivated: dialog.applyControls()
                }
                Label { id: locationInfo; opacity: 0.7 }
            }
            Label { text: "Incomplete" }
            RowLayout {
                spacing: 12
                AppCheckBox {
                    id: partial5
                    text: "5′ end"
                    ToolTip.text: "The annotation continues beyond its 5′ end (GenBank < or >)"
                    ToolTip.visible: hovered
                    onToggled: dialog.applyControls()
                }
                AppCheckBox {
                    id: partial3
                    text: "3′ end"
                    ToolTip.text: "The annotation continues beyond its 3′ end"
                    ToolTip.visible: hovered
                    onToggled: dialog.applyControls()
                }
            }
        }
        Label { text: "Qualifiers"; font.bold: true }
        ListView {
            id: list
            Layout.fillWidth: true
            Layout.preferredHeight: Math.max(100, contentHeight)
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
                    id: qualifierKey
                    objectName: "qualifierKey" + index
                    Accessible.name: "Qualifier key"
                    Layout.preferredWidth: 150
                    text: key
                    placeholderText: "key"
                    onTextEdited: quals.setProperty(index, "key", text)
                }
                TextField {
                    objectName: "qualifierValue" + index
                    Accessible.labelledBy: qualifierKey
                    Accessible.name: qualifierKey.text.length ? qualifierKey.text : "Qualifier value"
                    Layout.fillWidth: true
                    text: value
                    placeholderText: "value"
                    onTextEdited: quals.setProperty(index, "value", text)
                    onAccepted: dialog.save()
                }
                AppIconButton { iconName: "close"; description: "Remove qualifier"; onClicked: quals.remove(index) }
            }
        }
        RowLayout {
            AppButton { text: "Add qualifier"; onClicked: quals.append({ key: "", value: "", flag: false }) }
            Item { Layout.fillWidth: true }
            AppButton {
                text: "Delete annotation"
                visible: dialog.featureId >= 0
                onClicked: { dialog.controller.delete_feature(dialog.featureId); dialog.close() }
            }
            AppButton { text: "Cancel"; onClicked: dialog.reject() }
            AppButton { text: "Save"; highlighted: true; onClicked: dialog.save() }
        }
        AppErrorLabel {
            id: error
            objectName: "featureError"
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: dialog.controller ? dialog.controller.accent : "red"
            visible: text.length > 0
        }
    }
}
