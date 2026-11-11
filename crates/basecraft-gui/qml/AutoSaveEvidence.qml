import QtQuick
import QtQuick.Dialogs

Item {
    id: root
    property var controller: null
    required property var evidenceService
    required property var fileUrls
    property string target: ""
    property var owner: null
    property bool automatic: true
    property var valid: () => true
    property var chooseDestination: () => destination.open()
    property string dialogName: "autoSaveEvidenceDialog"
    signal finished(bool success)
    signal failed(string message)
    function inspect(saveIfClear) {
        if (!valid()) { dialog.close(); finished(false); return }
        const result = JSON.parse(controller.check_save(target))
        if (result.error) { dialog.errorText = result.error; failed(result.error); return }
        if (owner && (owner.session !== result.check.session || owner.document !== result.check.document)) { dialog.close(); return }
        owner = result.check
        dialog.check = result.check
        if (result.check.evidence.issues.length || result.check.evidence.conflict.analyses.length) dialog.open()
        else if (!saveIfClear) dialog.close()
        else perform("cancel", false)
    }
    function perform(policy, acknowledge) {
        if (!valid()) { dialog.close(); finished(false); return }
        const success = controller.save_confirmed(target, dialog.check.token, policy, acknowledge)
        dialog.close()
        if (!success) { inspect(false); dialog.errorText = controller.message; failed(controller.message) }
        finished(success)
    }
    Connections {
        target: root.controller
        enabled: root.automatic
        ignoreUnknownSignals: true
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
        objectName: root.dialogName
        onRetry: root.inspect(true)
        onSaveAs: root.chooseDestination()
        onRelinkOriginal: function(path) {
            const fresh = JSON.parse(root.controller.check_save(root.target)).check
            if (!fresh || !root.owner || fresh.session !== root.owner.session || fresh.document !== root.owner.document) { close(); return }
            for (const analysis of conflict.analyses || []) {
                if (analysis.direct !== false && !evidenceService.relink_run(analysis.run, path)) { errorText = evidenceService.message; return }
            }
            root.inspect(true)
        }
        onChoice: policy => root.perform(policy, acknowledgeIncomplete)
    }
}
