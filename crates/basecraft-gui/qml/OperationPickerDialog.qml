import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Basecraft

AppContentDialog {
    id: dialog
    title: "Installed operations"
    standardButtons: Dialog.Close
    initialFocusItem: operationChoice
    property string project: ""
    property string inputPath: ""
    property bool unsavedInput: false
    property var catalog: ({operations: []})
    readonly property var selected: operationChoice.currentIndex >= 0 ? catalog.operations[operationChoice.currentIndex] : null
    readonly property var result: JSON.parse(backend.outcome || "{}")
    signal openResult(string path)
    AppOperations { id: backend }
    Timer { interval: 200; repeat: true; running: backend.busy; onTriggered: backend.poll() }
    function show(project, path, dirty) {
        dialog.project = project
        inputPath = path
        unsavedInput = dirty
        catalog = JSON.parse(backend.catalog())
        open()
    }
    function start() {
        if (!selected || !selected.supported || unsavedInput) return
        const inputs = {}
        for (let i = 0; i < inputFields.count; ++i) {
            const row = inputFields.itemAt(i)
            inputs[row.role] = [{kind: "file", path: row.path}]
        }
        const parameters = {}
        for (let i = 0; i < parameterFields.count; ++i) {
            const row = parameterFields.itemAt(i)
            if (row.overridden || row.modelData.required) parameters[row.modelData.name] = row.value()
        }
        const destinations = {}
        for (let i = 0; i < outputFields.count; ++i) {
            const row = outputFields.itemAt(i)
            if (row.path.length) destinations[row.role] = row.path
        }
        backend.submit(project, JSON.stringify({operation: selected.operation, inputs: inputs, parameters: parameters, destinations: destinations, pins: selected.pins}))
    }
    body: ColumnLayout {
        spacing: 8
        Label { text: "Project: " + dialog.project; Layout.fillWidth: true; elide: Text.ElideMiddle }
        ComboBox {
            id: operationChoice
            Accessible.name: "Installed operation"
            Layout.fillWidth: true
            model: dialog.catalog.operations
            textRole: "title"
            enabled: !backend.busy
        }
        Label { visible: dialog.catalog.error !== undefined; text: dialog.catalog.error || ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { text: dialog.selected ? dialog.selected.description : "No typed operations are installed."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { visible: dialog.selected && !dialog.selected.supported; text: dialog.selected ? dialog.selected.limitation || "" : ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { visible: dialog.unsavedInput; text: "Save the current file before using the generic file-operation picker. Existing document tools can use unsaved snapshots."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Repeater {
            id: inputFields
            model: dialog.selected ? Object.keys(dialog.selected.inputs) : []
            delegate: AppFormRow {
                id: inputRow
                required property string modelData
                readonly property string role: modelData
                readonly property string path: input.text
                Label { text: inputRow.role; Layout.fillWidth: true }
                TextField { id: input; text: inputFields.count === 1 ? dialog.inputPath : ""; placeholderText: "File inside the project"; Accessible.name: inputRow.role + " input file"; enabled: !backend.busy; Layout.fillWidth: true }
            }
        }
        Repeater {
            id: parameterFields
            model: dialog.selected && dialog.selected.supported ? dialog.selected.fields : []
            delegate: ColumnLayout {
                id: fieldRow
                required property var modelData
                readonly property var schema: modelData.schema
                readonly property bool overridden: overrideValue.checked
                function value() {
                    if (schema.enum) return schema.enum[choice.currentIndex]
                    if (schema.type === "boolean") return flag.checked
                    if (schema.type === "integer") return number.value
                    return textValue.text
                }
                CheckBox { id: overrideValue; text: (fieldRow.modelData.required ? "Set " : "Override ") + fieldRow.modelData.name; checked: fieldRow.modelData.required; enabled: !backend.busy; Accessible.name: text }
                Label { visible: !fieldRow.overridden; text: fieldRow.schema.default !== undefined ? "Default: " + String(fieldRow.schema.default) : "Host or tool default"; Layout.fillWidth: true }
                ComboBox { id: choice; visible: !!fieldRow.schema.enum && fieldRow.overridden; model: fieldRow.schema.enum || []; enabled: !backend.busy; Layout.fillWidth: true; Accessible.name: fieldRow.modelData.name }
                CheckBox { id: flag; visible: !fieldRow.schema.enum && fieldRow.schema.type === "boolean" && fieldRow.overridden; checked: fieldRow.schema.default === true; enabled: !backend.busy; text: fieldRow.modelData.name; Accessible.name: text }
                SpinBox { id: number; visible: !fieldRow.schema.enum && fieldRow.schema.type === "integer" && fieldRow.overridden; from: fieldRow.schema.minimum === undefined ? 0 : fieldRow.schema.minimum; to: fieldRow.schema.maximum === undefined ? 2147483647 : Math.min(2147483647, fieldRow.schema.maximum); value: fieldRow.schema.default === undefined ? from : fieldRow.schema.default; editable: true; enabled: !backend.busy; Accessible.name: fieldRow.modelData.name }
                TextField { id: textValue; visible: !fieldRow.schema.enum && fieldRow.schema.type === "string" && fieldRow.overridden; text: fieldRow.schema.default || ""; enabled: !backend.busy; Layout.fillWidth: true; Accessible.name: fieldRow.modelData.name }
            }
        }
        Repeater {
            id: outputFields
            model: dialog.selected ? Object.keys(dialog.selected.outputs) : []
            delegate: AppFormRow {
                id: outputRow
                required property string modelData
                readonly property string role: modelData
                readonly property string path: destination.text
                Label { text: outputRow.role; Layout.fillWidth: true }
                TextField { id: destination; placeholderText: "Optional new file/folder; otherwise retain in analysis store"; enabled: !backend.busy; Accessible.name: outputRow.role + " output destination"; Layout.fillWidth: true }
            }
        }
        RowLayout {
            Button { text: "Run"; enabled: !backend.busy && dialog.selected && dialog.selected.supported && !dialog.unsavedInput; onClicked: dialog.start() }
            Button { text: "Cancel operation"; enabled: backend.busy; onClicked: backend.cancel() }
            Button { text: "Refresh installed operations"; enabled: !backend.busy; onClicked: dialog.catalog = JSON.parse(backend.catalog()) }
        }
        Label { text: backend.message; wrapMode: Text.Wrap; Layout.fillWidth: true; Accessible.name: "Operation status" }
        TextArea { visible: dialog.result.data !== undefined && dialog.result.data !== null; text: JSON.stringify(dialog.result.data, null, 2) || ""; readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap; Layout.fillWidth: true; Accessible.name: "Structured operation result" }
        Repeater {
            model: dialog.result.artifacts || []
            delegate: RowLayout {
                required property var modelData
                Label { text: modelData.role + ": " + modelData.path; elide: Text.ElideMiddle; Layout.fillWidth: true }
                Button { text: "Open"; onClicked: dialog.openResult(modelData.path) }
            }
        }
    }
}
