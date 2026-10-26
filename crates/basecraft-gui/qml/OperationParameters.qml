import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ColumnLayout {
    id: form
    property var schema: ({type: "object", properties: {}})
    property var initialValue: ({})
    readonly property var alternatives: schema.oneOf || []
    function title(name) { return (schema.properties[name].title || name.replace(/_/g, " ")) }
    function alternativeIndex(name) {
        for (let i = 0; i < alternatives.length; ++i)
            if ((alternatives[i].required || []).indexOf(name) >= 0) return i
        return -1
    }
    function value() {
        const result = {}
        for (let i = 0; i < fields.count; ++i) {
            const row = fields.itemAt(i)
            if (row.included) {
                if (!row.editor.item) throw new Error("Settings are still loading")
                try { result[row.modelData] = row.editor.item.value() }
                catch (error) { throw new Error(form.title(row.modelData) + ": " + error.message) }
            }
        }
        return result
    }
    ComboBox {
        id: alternative
        objectName: "parameterAlternative"
        visible: form.alternatives.length > 0
        Layout.fillWidth: true
        Accessible.name: "Parameter choice"
        model: form.alternatives.map(option => option.title || option.required.map(name => form.title(name)).join(" + "))
    }
    Repeater {
        id: fields
        model: Object.keys(form.schema.properties || {})
        delegate: ColumnLayout {
            id: row
            required property string modelData
            readonly property var field: form.schema.properties[modelData]
            readonly property int option: form.alternativeIndex(modelData)
            readonly property bool mandatory: (form.schema.required || []).indexOf(modelData) >= 0 || option >= 0
            readonly property bool included: visible && (mandatory || includeField.checked)
            property alias editor: editor
            visible: option < 0 || option === alternative.currentIndex
            Layout.fillWidth: true
            Label { visible: row.mandatory; text: form.title(row.modelData); Layout.fillWidth: true; wrapMode: Text.Wrap }
            CheckBox {
                id: includeField
                objectName: "include_" + row.modelData
                visible: !row.mandatory
                text: "Set " + form.title(row.modelData)
                checked: form.initialValue && form.initialValue[row.modelData] !== undefined
                Accessible.name: text
            }
            Label { visible: !!row.field.description && row.included; text: row.field.description || ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
            Loader {
                id: editor
                Layout.fillWidth: true
                active: row.included
                visible: active
                function reload() {
                    if (active) setSource("OperationParameterValue.qml", {
                        schema: row.field,
                        fieldName: row.modelData,
                        initialValue: form.initialValue ? form.initialValue[row.modelData] : undefined
                    })
                }
                onActiveChanged: reload()
                Component.onCompleted: reload()
            }
        }
    }
}
