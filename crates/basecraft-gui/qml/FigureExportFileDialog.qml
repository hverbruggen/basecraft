import QtQuick
import QtQuick.Dialogs

// Where an SVG or PNG figure goes; writes it through the document's controller.
FileDialog {
    id: svgDialogItem
    required property var app
    onVisibleChanged: if (visible) currentFolder = app.activeFolder()
    property var controller: null
    property string sourcePath: ""
    property string format: "svg"
    property int scope: 0
    property bool publication: false
    property int rasterWidth: 1500
    property bool alignment: false
    property bool includeSelection: false
    title: format === "png" ? "Export PNG" : "Export SVG"
    fileMode: FileDialog.SaveFile
    nameFilters: format === "png" ? ["PNG images (*.png)"] : ["SVG figures (*.svg)"]
    onAccepted: {
        if (!controller) return
        if (alignment && format === "png") controller.export_figure_png_selection(app.shell.local_file_path(selectedFile), scope, publication, rasterWidth, includeSelection)
        else if (alignment) controller.export_figure_selection(app.shell.local_file_path(selectedFile), scope, publication, includeSelection)
        else if (format === "png") controller.export_figure_png(app.shell.local_file_path(selectedFile), scope, publication, rasterWidth)
        else controller.export_figure(app.shell.local_file_path(selectedFile), scope, publication)
    }
}
