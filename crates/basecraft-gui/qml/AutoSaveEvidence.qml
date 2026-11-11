import QtQuick
import QtQuick.Dialogs

Item {
    id: root
    required property var controller
    required property var evidenceService
    required property var fileUrls
    property string target: ""
    property var owner: null
    function inspect(saveIfClear) {
        const result = JSON.parse(controller.check_save(target))
        if (result.error) { dialog.errorText = result.error; return }
        if (owner && (owner.session !== result.check.session || owner.document !== result.check.document)) { dialog.close(); return }
        owner = result.check
        dialog.check = result.check
        if (result.check.evidence.issues.length || result.check.evidence.conflict.analyses.length) dialog.open()
        else if (!saveIfClear) dialog.close()
        else if (controller.save_confirmed(target, result.check.token, "cancel", false)) dialog.close()
        else dialog.errorText = controller.message
    }
    Connections {
        target: root.controller
        function onSaveQuestion() {
            root.target = ""
            root.owner = JSON.parse(root.controller.check_save("")).check || null
            Qt.callLater(() => root.inspect(true))
        }
    }
    FileDialog {
        id: destination
        fileMode: FileDialog.SaveFile
        onAccepted: { root.target = root.fileUrls.local_file_path(selectedFile); root.inspect(true) }
    }
    AnalysisEvidenceDialog {
        id: dialog
        objectName: "autoSaveEvidenceDialog"
        onRetry: root.inspect(true)
        onSaveAs: destination.open()
        onRelinkOriginal: function(path) {
            const fresh = JSON.parse(root.controller.check_save(root.target)).check
            if (!fresh || !root.owner || fresh.session !== root.owner.session || fresh.document !== root.owner.document) { close(); return }
            for (const analysis of conflict.analyses || []) {
                if (analysis.direct !== false && !evidenceService.relink_run(analysis.run, path)) { errorText = evidenceService.message; return }
            }
            root.inspect(true)
        }
        onChoice: function(policy) {
            if (root.controller.save_confirmed(root.target, check.token, policy, acknowledgeIncomplete)) close()
            else { root.inspect(false); errorText = root.controller.message }
        }
    }
}
