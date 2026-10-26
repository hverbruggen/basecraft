import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Basecraft

AppContentDialog {
    id: dialog
    title: operationId.length ? "Rate smoothing" : "Installed operations"
    property string operationId: ""
    property var allOperations: []
    standardButtons: Dialog.Close
    initialFocusItem: operationId.length ? runButton : operationChoice
    property string project: ""
    property string inputPath: ""
    property bool unsavedInput: false
    property var catalog: ({operations: []})
    readonly property var selected: operationChoice.currentIndex >= 0 ? catalog.operations[operationChoice.currentIndex] : null
    readonly property var operationOutcome: JSON.parse(backend.outcome || "{}")
    signal openResult(string path)
    AppOperations { id: backend }
    Timer { interval: 200; repeat: true; running: backend.busy; onTriggered: backend.poll() }
    function show(project, path, dirty) {
        dialog.project = project
        inputPath = path
        unsavedInput = dirty
        operationId = ""
        refreshCatalog()
        open()
    }
    function refreshCatalog() {
        const data = JSON.parse(backend.catalog())
        allOperations = data.operations || []
        if (operationId.length) {
            data.operations = allOperations.filter(op => op.operation === operationId)
            if (!data.operations.length && !data.error) data.error = "The Sanderson operation is not installed."
        }
        catalog = data
        operationChoice.currentIndex = data.operations.length ? 0 : -1
    }
    function showOperation(project, path, dirty, id) {
        dialog.project = project
        inputPath = path
        unsavedInput = dirty
        operationId = id
        refreshCatalog()
        open()
    }
    property string formError: ""
    function start() {
        formError = ""
        if (!selected || !selected.supported || unsavedInput) return
        const inputs = {}
        for (let i = 0; i < inputFields.count; ++i) {
            const row = inputFields.itemAt(i)
            inputs[row.role] = [{kind: "file", path: row.path}]
        }
        let parameters
        try { parameters = parameterForm.item.value() }
        catch (error) { formError = error.message; return }
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
            visible: !dialog.operationId.length
            Accessible.name: "Installed operation"
            Layout.fillWidth: true
            model: dialog.catalog.operations
            textRole: "title"
            enabled: !backend.busy
        }
        Label { visible: dialog.catalog.error !== undefined; text: dialog.catalog.error || ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { text: dialog.selected ? dialog.selected.description : "No typed operations are installed."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { visible: dialog.selected && !dialog.selected.supported; text: dialog.selected ? dialog.selected.limitation || "" : ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { visible: dialog.unsavedInput; text: "Save the current tree before running this file operation."; wrapMode: Text.Wrap; Layout.fillWidth: true }
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
        Loader {
            id: parameterForm
            Layout.fillWidth: true
            active: dialog.selected && dialog.selected.supported
            function reload() {
                source = ""
                dialog.formError = ""
                if (active) setSource("OperationParameters.qml", {schema: dialog.selected.parameters})
            }
            onActiveChanged: reload()
            Connections { target: dialog; function onSelectedChanged() { parameterForm.reload() } }
        }
        Label { visible: dialog.formError.length > 0; text: dialog.formError; wrapMode: Text.Wrap; Layout.fillWidth: true; Accessible.name: "Invalid settings" }
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
            Button { id: runButton; text: "Run"; enabled: !backend.busy && dialog.selected && dialog.selected.supported && !dialog.unsavedInput; onClicked: dialog.start() }
            Button { text: "Cancel operation"; enabled: backend.busy; onClicked: backend.cancel() }
            Button { text: "Refresh installed operations"; enabled: !backend.busy; onClicked: dialog.refreshCatalog() }
        }
        Label { text: backend.message; wrapMode: Text.Wrap; Layout.fillWidth: true; Accessible.name: "Operation status" }
        TextArea { visible: dialog.operationOutcome.data !== undefined && dialog.operationOutcome.data !== null; text: JSON.stringify(dialog.operationOutcome.data, null, 2) || ""; readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap; Layout.fillWidth: true; Accessible.name: "Structured operation result" }
        Repeater {
            model: dialog.operationOutcome.artifacts || []
            delegate: RowLayout {
                required property var modelData
                Label { text: modelData.role + ": " + modelData.path; elide: Text.ElideMiddle; Layout.fillWidth: true }
                Button { text: "Open"; onClicked: dialog.openResult(modelData.path) }
            }
        }
    }
}
