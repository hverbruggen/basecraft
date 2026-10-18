import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// A plugin's settings form, drawn from the fields its manifest declares
// (`[[forms]]` in plugin.toml). Fields that do not apply are greyed rather
// than hidden, so the others keep their places.
AppContentDialog {
    id: dialog
    objectName: "toolSettingsDialog"
    property var target: null
    property string plugin: ""
    property var form: ({fields: []})
    // Field name to value, for every field.
    property var values: ({})
    title: form.title || ""
    standardButtons: Dialog.Ok | Dialog.Cancel

    function show(target, plugin, id) {
        const loaded = JSON.parse(target.tool_form(plugin, id))
        if (loaded.error) {
            target.message_kind = "error"
            target.message = loaded.error
            return
        }
        dialog.target = target
        dialog.plugin = plugin
        const start = {}
        for (const field of loaded.fields) start[field.name] = field.default
        values = start
        form = loaded
        const first = fields.itemAt(0)
        initialFocusItem = first ? first.control : null
        open()
    }
    // `name == value` or `name != value`, compared as text.
    function holds(condition) {
        const match = /^\s*(\w+)\s*(==|!=)\s*(.*?)\s*$/.exec(condition || "")
        if (!match) return false
        const value = values[match[1]] === undefined ? "" : String(values[match[1]])
        return (value === match[3]) === (match[2] === "==")
    }
    function applies(field) { return !field.when || holds(field.when) }
    function forced(field) {
        for (const rule of field.value_when || []) if (holds(rule[0])) return rule[1]
        return undefined
    }
    function minimum(field) {
        let min = field.min === null || field.min === undefined ? 0 : field.min
        for (const rule of field.min_when || []) if (holds(rule[0])) min = Math.max(min, rule[1])
        return min
    }
    function value(field) {
        const fixed = forced(field)
        return fixed === undefined ? values[field.name] : fixed
    }
    function set(name, value) {
        const next = Object.assign({}, values)
        next[name] = value
        values = next
    }

    onOpened: standardButton(Dialog.Ok).text = form.button || "Run"
    onAccepted: if (target && !target.busy) target.run_form(plugin, form.id, JSON.stringify(values))

    FontMetrics { id: labelMetrics; font: dialog.font }
    // One width for every label, so the fields line up.
    readonly property real labelWidth: Math.ceil(Math.max(0, ...form.fields.filter(f => f.type !== "boolean").map(f => labelMetrics.advanceWidth(f.label))))

    body: ColumnLayout {
        spacing: 8
        Repeater {
            id: fields
            model: dialog.form.fields
            // A label beside its field, above it when the two do not fit.
            delegate: AppFormRow {
                id: row
                required property var modelData
                readonly property string kind: modelData.type
                readonly property bool live: dialog.applies(modelData) && dialog.forced(modelData) === undefined
                readonly property Item control: kind === "choice" ? choice : kind === "integer" ? number : toggle
                objectName: "field-" + modelData.name
                rowSpacing: 2
                // Every label takes the widest label's room.
                Item {
                    visible: row.kind !== "boolean"
                    implicitWidth: dialog.labelWidth
                    implicitHeight: label.implicitHeight
                    // Shrinks with the dialog; beside its field, no wider than the widest label.
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Layout.maximumWidth: row.columns === 1 ? Number.POSITIVE_INFINITY : dialog.labelWidth
                    Label {
                        id: label
                        anchors.fill: parent
                        verticalAlignment: Text.AlignVCenter
                        text: row.modelData.label
                        enabled: row.live
                        elide: Text.ElideRight
                    }
                }
                AppComboBox {
                    id: choice
                    visible: row.kind === "choice"
                    objectName: row.kind === "choice" ? row.modelData.name : ""
                    enabled: row.live
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    model: (row.modelData.choices || []).map(c => c[1])
                    currentIndex: (row.modelData.choices || []).findIndex(c => c[0] === dialog.value(row.modelData))
                    onActivated: index => dialog.set(row.modelData.name, row.modelData.choices[index][0])
                    Accessible.labelledBy: label
                    Accessible.name: label.text
                }
                AppSpinBox {
                    id: number
                    visible: row.kind === "integer"
                    objectName: row.kind === "integer" ? row.modelData.name : ""
                    enabled: row.live
                    from: dialog.minimum(row.modelData)
                    to: row.modelData.max === null || row.modelData.max === undefined ? 2147483647 : row.modelData.max
                    value: row.kind === "integer" ? dialog.value(row.modelData) : 0
                    editable: true
                    // As wide as its largest value, not the column.
                    Layout.minimumWidth: 0
                    onValueModified: dialog.set(row.modelData.name, value)
                    // A raised minimum raises the value with it.
                    onFromChanged: if (visible && dialog.values[row.modelData.name] < from) dialog.set(row.modelData.name, from)
                    Accessible.labelledBy: label
                    Accessible.name: label.text
                }
                AppCheckBox {
                    id: toggle
                    visible: row.kind === "boolean"
                    objectName: row.kind === "boolean" ? row.modelData.name : ""
                    enabled: row.live
                    text: row.modelData.label
                    checked: dialog.value(row.modelData) === true
                    onToggled: dialog.set(row.modelData.name, checked)
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                }
            }
        }
        Label {
            visible: text.length > 0
            text: dialog.form.note || ""
            wrapMode: Text.Wrap
            opacity: 0.75
            Layout.preferredWidth: 1
            Layout.fillWidth: true
        }
    }
}
