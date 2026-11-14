import QtQuick
import QtQuick.Controls.Basic

// What applies to the alignment cells, rows or columns under the pointer.
Menu {
    id: alnMenu
    required property var app
    required property var clipboard
    required property var renameDialog
    property var info: ({ region: "grid", rows: 1, columns: 1, rowName: "" })
    readonly property bool cells: info.region === "grid"
    readonly property bool rows: info.region === "names" || info.region === "corner"
    readonly property bool columns: info.region === "header"
    component AlnItem: MenuItem { height: visible ? implicitHeight : 0 }

    AlnItem { text: "Insert gap"; visible: alnMenu.cells; onTriggered: app.current.insert_gap() }
    AlnItem { text: "Remove gap to the left"; visible: alnMenu.cells; onTriggered: app.current.remove_gap() }
    AlnItem { text: "Delete " + alnMenu.info.rows + " × " + alnMenu.info.columns + " bases"; visible: alnMenu.cells; onTriggered: app.current.delete_selection() }
    AlnItem { text: "Rename “" + alnMenu.info.rowName + "”…"; visible: alnMenu.info.rows === 1 && !alnMenu.columns; onTriggered: renameDialog.dialog().askRow() }
    AlnItem { text: "Move to top"; visible: alnMenu.rows; onTriggered: app.current.rows_to_top() }
    AlnItem { text: alnMenu.info.rows === 1 ? "Delete row" : "Delete " + alnMenu.info.rows + " rows"; visible: alnMenu.rows; onTriggered: app.current.remove_rows() }
    AlnItem { text: "Insert gap column"; visible: alnMenu.columns; onTriggered: app.current.insert_column() }
    AlnItem { text: alnMenu.info.columns === 1 ? "Delete column" : "Delete " + alnMenu.info.columns + " columns"; visible: alnMenu.columns; onTriggered: app.current.delete_columns() }
    AlnItem { text: "Strip gap-only columns"; visible: alnMenu.columns || alnMenu.rows; onTriggered: app.current.remove_gap_columns() }
    AlnItem { text: "BLAST selected rows at NCBI"; onTriggered: app.beginBlast("rows") }
    AlnItem { text: "Copy as FASTA"; onTriggered: clipboard.put(app.current.selection_text()) }
}
