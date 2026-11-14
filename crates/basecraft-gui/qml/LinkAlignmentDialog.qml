import QtQuick
import QtQuick.Dialogs

// Choose the alignment whose rows a tree's tips name; refused if the tree changed meanwhile.
FileDialog {
    id: linkAlignmentFileItem
    required property var app
    property var editController: null
    property string editRevision: ""
    function ask(target) {
        editController = target
        editRevision = target.record_revision()
        open()
    }
    onVisibleChanged: if (visible) currentFolder = app.activeFolder()
    title: "Link an alignment to this tree"
    nameFilters: ["FASTA alignments (*.fasta *.fa *.fas *.afa)"]
    onAccepted: {
        if (!editController) return
        if (editController.record_revision() !== editRevision) {
            editController.message_kind = "error"
            editController.message = "The tree changed. Choose the linked alignment again."
            return
        }
        editController.link_alignment(app.shell.local_file_path(selectedFile))
    }
}
