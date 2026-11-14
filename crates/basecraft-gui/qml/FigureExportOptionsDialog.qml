import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// The region and colours of an SVG or PNG figure, asked before the file name.
AppContentDialog {
    id: svgOptionsItem
    required property var app
    required property var fileDialog
    onAboutToShow: {
        svgScope.currentIndex = app.treeOpen ? 2 : 0
        svgPublication.checked = false
        svgSelection.checked = false
    }
    objectName: "figureExportOptions"
    initialFocusItem: svgScope
    property string format: "svg"
    property bool alignment: false
    title: format === "png" ? "Export PNG" : "Export editable SVG"
    standardButtons: Dialog.Ok | Dialog.Cancel
    body: ColumnLayout {
        Label { id: svgScopeLabel; text: "Region" }
        AppComboBox { id: svgScope; objectName: "svgScope"; Accessible.labelledBy: svgScopeLabel; Accessible.name: svgScopeLabel.text; model: ["Visible view", "Selection", "Complete figure"] }
        AppCheckBox { id: svgPublication; text: "Publication colours (white background)" }
        AppCheckBox { id: svgSelection; objectName: "svgSelection"; text: "Include selection highlighting"; visible: svgOptionsItem.alignment }
        RowLayout {
            visible: svgOptionsItem.format === "png"
            Label { id: pngWidthLabel; text: "PNG width (pixels)"; visible: svgOptionsItem.format === "png" }
            AppSpinBox {
                id: pngWidth
                objectName: "pngWidth"
                visible: svgOptionsItem.format === "png"
                from: 1
                to: 10000
                value: 1500
                editable: true
                Accessible.labelledBy: pngWidthLabel
                Accessible.name: pngWidthLabel.text
            }
        }
    }
    onAccepted: {
        fileDialog.dialog().format = svgOptionsItem.format
        fileDialog.dialog().scope = svgScope.currentIndex
        fileDialog.dialog().publication = svgPublication.checked
        fileDialog.dialog().alignment = svgOptionsItem.alignment
        fileDialog.dialog().includeSelection = svgSelection.checked
        fileDialog.dialog().rasterWidth = pngWidth.value
        fileDialog.dialog().selectedFile = app.shell.local_file_url(fileDialog.dialog().sourcePath.replace(/\.[^./]*$/, "") + "." + fileDialog.dialog().format)
        fileDialog.dialog().open()
    }
}
