import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ColumnLayout {
    id: valueEditor
    property var schema: ({type: "string"})
    property string fieldName: ""
    property var initialValue
    readonly property var startingValue: initialValue !== undefined ? initialValue : schema.default
    function value() {
        if (schema.enum) {
            if (choice.currentIndex < 0) throw new Error("Choose a value")
            return schema.enum[choice.currentIndex]
        }
        if (schema.type === "boolean") return flag.checked
        if (schema.type === "object") {
            if (!objectFields.item) throw new Error("Settings are still loading")
            return objectFields.item.value()
        }
        if (schema.type === "array") {
            const result = []
            for (let i = 0; i < entries.count; ++i) {
                const child = entries.itemAt(i).editor.item
                if (!child) throw new Error("Settings are still loading")
                result.push(child.value())
            }
            return result
        }
        if (schema.type === "number" || schema.type === "integer") {
            const text = scalar.text.trim()
            const number = Number(text)
            if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(text) || !isFinite(number))
                throw new Error("Enter a finite number")
            if (schema.type === "integer" && !Number.isSafeInteger(number)) throw new Error("Enter a whole number")
            if (schema.minimum !== undefined && number < schema.minimum) throw new Error("Minimum is " + schema.minimum)
            if (schema.maximum !== undefined && number > schema.maximum) throw new Error("Maximum is " + schema.maximum)
            if (schema.exclusiveMinimum !== undefined && number <= schema.exclusiveMinimum) throw new Error("Must be greater than " + schema.exclusiveMinimum)
            if (schema.exclusiveMaximum !== undefined && number >= schema.exclusiveMaximum) throw new Error("Must be less than " + schema.exclusiveMaximum)
            return number
        }
        return scalar.text
    }
    ComboBox {
        id: choice
        objectName: "value_" + valueEditor.fieldName
        visible: !!valueEditor.schema.enum
        model: valueEditor.schema.enum || []
        currentIndex: valueEditor.startingValue === undefined ? 0 : model.indexOf(valueEditor.startingValue)
        Accessible.name: valueEditor.fieldName
        Layout.fillWidth: true
    }
    CheckBox {
        id: flag
        visible: !valueEditor.schema.enum && valueEditor.schema.type === "boolean"
        checked: valueEditor.startingValue === true
        text: valueEditor.fieldName.replace(/_/g, " ")
        Accessible.name: text
    }
    TextField {
        id: scalar
        objectName: "text_" + valueEditor.fieldName
        visible: !valueEditor.schema.enum && ["number", "integer", "string"].indexOf(valueEditor.schema.type) >= 0
        text: valueEditor.startingValue === undefined ? "" : String(valueEditor.startingValue)
        Accessible.name: valueEditor.fieldName
        Layout.fillWidth: true
        selectByMouse: true
    }
    Loader {
        id: objectFields
        active: valueEditor.schema.type === "object" && !valueEditor.schema.enum
        visible: active
        Layout.fillWidth: true
        Component.onCompleted: if (active) setSource("OperationParameters.qml", {schema: valueEditor.schema, initialValue: valueEditor.startingValue || {}})
    }
    ListModel { id: items }
    Component.onCompleted: {
        if (schema.type !== "array") return
        const values = startingValue || []
        const count = Math.max(values.length, schema.minItems || 0)
        for (let i = 0; i < count; ++i) items.append({seed: JSON.stringify(values[i] === undefined ? null : values[i])})
    }
    Repeater {
        id: entries
        model: items
        delegate: ColumnLayout {
            id: entry
            required property int index
            required property string seed
            property alias editor: child
            Layout.fillWidth: true
            Loader {
                id: child
                Layout.fillWidth: true
                Component.onCompleted: setSource("OperationParameterValue.qml", {schema: valueEditor.schema.items, fieldName: valueEditor.fieldName + " " + (entry.index + 1), initialValue: JSON.parse(entry.seed) === null ? undefined : JSON.parse(entry.seed)})
            }
            Button {
                text: "Remove item"
                enabled: items.count > (valueEditor.schema.minItems || 0)
                onClicked: items.remove(entry.index)
            }
        }
    }
    Button {
        objectName: "add_" + valueEditor.fieldName
        visible: valueEditor.schema.type === "array" && !valueEditor.schema.enum
        text: "Add item"
        enabled: items.count < (valueEditor.schema.maxItems === undefined ? 128 : valueEditor.schema.maxItems)
        onClicked: items.append({seed: "null"})
    }
}
