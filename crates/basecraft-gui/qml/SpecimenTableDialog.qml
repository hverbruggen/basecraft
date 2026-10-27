import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The project's specimen table for small fixes: one row per specimen, its
// current taxon and its collection columns. Every change is one undoable
// edit, saved at once by the controller. Large or messy changes go through
// an import, which is reviewed first.
AppContentDialog {
    id: dialog
    objectName: "specimenTable"
    title: "Specimen table"
    standardButtons: Dialog.Close
    initialFocusItem: table.available ? newSpecimen : createButton
    // MetadataController.table, parsed.
    property var table: ({ available: false, columns: [], rows: [], addable: [], notices: [] })
    property bool canUndo: false
    property bool canRedo: false
    property string message: ""
    property string messageKind: ""
    signal createRequested()
    signal cellEdited(string specimen, string column, string text)
    signal taxonEdited(string specimen, string name)
    signal specimenAdded(string id)
    signal specimenRemoved(string id)
    signal columnAdded(string name)
    signal undoRequested()
    signal redoRequested()

    FontMetrics { id: cellMetrics; font: dialog.font }
    readonly property real idWidth: cellMetrics.averageCharacterWidth * 14
    readonly property real cellWidth: cellMetrics.averageCharacterWidth * 18

    body: ColumnLayout {
        spacing: 8
        Label {
            visible: !dialog.table.available
            text: "This project has no specimen table yet. Create one, or import specimens from FASTA headers or a spreadsheet."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        AppButton {
            id: createButton
            objectName: "createSpecimenTable"
            visible: !dialog.table.available
            text: "Create specimen table"
            onClicked: dialog.createRequested()
        }
        Repeater {
            model: dialog.table.notices || []
            delegate: Label {
                required property string modelData
                text: modelData
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
        AppErrorLabel {
            objectName: "specimenTableMessage"
            visible: text.length > 0
            text: dialog.message
            color: dialog.messageKind === "error" ? "#c0392b" : palette.windowText
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        AppFormRow {
            visible: dialog.table.available
            RowLayout {
                spacing: 6
                TextField {
                    id: newSpecimen
                    objectName: "newSpecimenId"
                    placeholderText: "Specimen ID"
                    Accessible.name: "New specimen ID"
                    Layout.preferredWidth: dialog.idWidth
                    onAccepted: addSpecimen.clicked()
                }
                AppButton {
                    id: addSpecimen
                    text: "Add specimen"
                    enabled: newSpecimen.text.trim().length > 0
                    onClicked: { dialog.specimenAdded(newSpecimen.text.trim()); newSpecimen.clear() }
                }
            }
            RowLayout {
                spacing: 6
                AppComboBox {
                    id: newColumn
                    objectName: "newColumn"
                    Accessible.name: "Column to add"
                    editable: true
                    model: (dialog.table.addable || []).map(c => c.name)
                }
                AppButton {
                    text: "Add column"
                    enabled: newColumn.editText.trim().length > 0
                    onClicked: dialog.columnAdded(newColumn.editText.trim())
                }
            }
            RowLayout {
                spacing: 6
                AppButton { text: "Undo"; enabled: dialog.canUndo; onClicked: dialog.undoRequested() }
                AppButton { text: "Redo"; enabled: dialog.canRedo; onClicked: dialog.redoRequested() }
            }
        }
        Label {
            visible: dialog.table.available
            text: dialog.table.rows.length + " specimens · " + (dialog.table.taxa || 0) + " taxa · "
                + (dialog.table.links || 0) + " linked records. Enter values as text: dates as YYYY, YYYY-MM or YYYY-MM-DD, coordinates in decimal degrees, or an INSDC reason such as \"not collected\"."
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        ScrollView {
            id: grid
            visible: dialog.table.available && dialog.table.rows.length > 0
            Layout.fillWidth: true
            implicitHeight: rows.implicitHeight + 16
            contentWidth: rows.implicitWidth
            ScrollBar.vertical.policy: ScrollBar.AlwaysOff
            ColumnLayout {
                id: rows
                spacing: 2
                RowLayout {
                    spacing: 4
                    Label { text: "Specimen"; font.bold: true; Layout.preferredWidth: dialog.idWidth }
                    Label { text: "Taxon"; font.bold: true; Layout.preferredWidth: dialog.cellWidth }
                    Repeater {
                        model: dialog.table.columns
                        delegate: Label {
                            required property var modelData
                            text: modelData.name
                            font.bold: true
                            elide: Text.ElideRight
                            Layout.preferredWidth: dialog.cellWidth
                            Accessible.description: modelData.description || modelData.type
                        }
                    }
                }
                Repeater {
                    model: dialog.table.rows
                    delegate: RowLayout {
                        id: specimenRow
                        required property var modelData
                        spacing: 4
                        Label {
                            text: specimenRow.modelData.id
                            elide: Text.ElideRight
                            Layout.preferredWidth: dialog.idWidth
                            ToolTip.text: specimenRow.modelData.links + " linked records, "
                                + specimenRow.modelData.identifications + " identifications"
                        }
                        TextField {
                            objectName: "taxon_" + specimenRow.modelData.id
                            text: specimenRow.modelData.taxon
                            placeholderText: "Not identified"
                            Accessible.name: "Taxon of " + specimenRow.modelData.id
                            Layout.preferredWidth: dialog.cellWidth
                            onEditingFinished: if (text.trim() !== specimenRow.modelData.taxon && text.trim().length)
                                dialog.taxonEdited(specimenRow.modelData.id, text.trim())
                        }
                        Repeater {
                            model: dialog.table.columns
                            delegate: TextField {
                                required property var modelData
                                readonly property string stored: specimenRow.modelData.cells[modelData.name] || ""
                                objectName: "cell_" + specimenRow.modelData.id + "_" + modelData.name
                                text: stored
                                Accessible.name: modelData.name + " of " + specimenRow.modelData.id
                                Layout.preferredWidth: dialog.cellWidth
                                onEditingFinished: if (text.trim() !== stored)
                                    dialog.cellEdited(specimenRow.modelData.id, modelData.name, text)
                            }
                        }
                        AppButton {
                            text: "Remove"
                            Accessible.name: "Remove specimen " + specimenRow.modelData.id
                            enabled: specimenRow.modelData.links === 0 && specimenRow.modelData.identifications === 0
                            onClicked: dialog.specimenRemoved(specimenRow.modelData.id)
                        }
                    }
                }
            }
        }
    }
}
