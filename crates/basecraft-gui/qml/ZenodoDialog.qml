import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

AppFormDialog {
    id: dialog
    objectName: "zenodoDialog"
    title: "Zenodo draft"
    standardButtons: Dialog.Cancel
    initialFocusItem: titleField
    required property var backend
    property string project: ""
    property var preview: ({})
    property string selectionPath: ""
    property string resumePath: ""
    property var receipt: ({})
    property string error: ""
    property string phase: ""
    property bool attempted: false
    property var submittedMetadata: ({})
    property var openExternal: url => Qt.openUrlExternally(url)
    readonly property bool ready: preview.files !== undefined && selectionPath.length > 0
    readonly property bool rejectedBeforeCreation: attempted && resumePath.length > 0 && receipt.status === "not_sent" && receipt.draft_id === null
    readonly property bool canRetry: attempted && resumePath.length > 0 && (receipt.draft_id > 0 || rejectedBeforeCreation) && receipt.status !== "draft_ready"
    readonly property bool frozenMetadata: attempted && (backend.busy || !rejectedBeforeCreation)
    signal chooseFiles()

    function filesInput(paths) { return paths.map(path => ({kind: "file", path: path})) }
    function request(operation, inputs, parameters) {
        return JSON.stringify({operation: "basecraft.zenodo/" + operation + "@1", inputs: inputs, parameters: parameters})
    }
    function show(paths, root, defaults) {
        if (backend.busy) { open(); return }
        project = root
        preview = ({}); receipt = ({}); selectionPath = ""; resumePath = ""; error = ""; attempted = false
        titleField.text = defaults && defaults.title || titleField.text
        creators.text = defaults && defaults.creators || creators.text
        token.text = ""
        phase = "prepare"
        open()
        if (!backend.submit(project, request("prepare", {files: filesInput(paths)}, {}))) error = backend.message
    }
    function settings() {
        return {title: titleField.text.trim(), description: description.text.trim(), creators: creators.text.trim(),
            license: license.text.trim(), access_right: access.currentIndex === 0 ? "open" : "closed",
            service: service.currentIndex === 0 ? "sandbox" : "production"}
    }
    function start() {
        error = ""
        if (!ready || backend.busy || (attempted && !canRetry)) return
        const parameters = frozenMetadata ? submittedMetadata : settings()
        if (!parameters.title || !parameters.description || !parameters.creators || !parameters.license || !token.text.trim()) {
            error = "Enter a title, description, creators, license and access token."
            return
        }
        const inputs = {files: filesInput(preview.files.map(file => file.snapshot)), selection: filesInput([selectionPath])}
        if (attempted) inputs.resume = filesInput([resumePath])
        submittedMetadata = parameters
        phase = "draft"
        if (backend.submit_zenodo(project, request("draft", inputs, parameters), token.text)) attempted = true
        else error = backend.message
        token.text = ""
    }
    function completed() {
        const outcome = JSON.parse(backend.outcome || "{}")
        if (phase === "prepare") {
            if (outcome.state === "completed" && outcome.data && outcome.artifacts && outcome.artifacts.length) {
                preview = outcome.data
                selectionPath = outcome.artifacts[0].path
            } else error = (outcome.errors || []).map(e => e.message).join("\n") || backend.message
        } else {
            receipt = outcome.data || {}
            const artifacts = outcome.artifacts || []
            resumePath = artifacts.length ? artifacts[0].path : outcome.evidence ? outcome.evidence + "/pending/receipt" : ""
            error = (outcome.errors || []).map(e => e.message).join("\n")
            if (!receipt.status) error += "\nInspect the run evidence before retrying: " + (outcome.evidence || "unavailable")
        }
    }
    onRejected: { token.text = ""; if (backend.busy) backend.cancel() }
    onClosed: token.text = ""

    Label {
        Kirigami.FormData.isSection: true
        text: "Upload saved file snapshots to a draft. Final publication requires review and confirmation on Zenodo."
        wrapMode: Text.Wrap
        Layout.preferredWidth: dialog.font.pixelSize * 37
    }
    ScrollView {
        Kirigami.FormData.label: "Selected bytes:"
        Layout.preferredWidth: dialog.font.pixelSize * 37
        Layout.preferredHeight: dialog.font.pixelSize * 9
        clip: true
        TextArea {
            objectName: "zenodoFiles"
            text: dialog.ready ? dialog.preview.files.map(f => f.name + " — " + f.size + " bytes\nSHA-256: " + f.sha256).join("\n\n")
                + "\n\nTotal: " + dialog.preview.total_bytes + " bytes. Later source edits do not change these snapshots." : "Preparing the selected files…"
            readOnly: true; selectByMouse: true; wrapMode: TextEdit.WrapAnywhere
            Accessible.name: "Selected filenames, byte sizes and SHA-256 checksums"
        }
    }
    AppButton { text: "Choose &files"; enabled: !dialog.backend.busy; onClicked: dialog.chooseFiles() }
    TextField { id: titleField; objectName: "zenodoTitle"; Kirigami.FormData.label: "&Title:"; enabled: !dialog.frozenMetadata; Accessible.name: "Title" }
    TextArea { id: description; objectName: "zenodoDescription"; Kirigami.FormData.label: "&Description:"; enabled: !dialog.frozenMetadata; clip: true; wrapMode: TextEdit.Wrap; Layout.preferredWidth: dialog.font.pixelSize * 37; Layout.preferredHeight: dialog.font.pixelSize * 5; Accessible.name: "Description" }
    TextArea { id: creators; objectName: "zenodoCreators"; Kirigami.FormData.label: "Creator &names:"; placeholderText: "Family name, Given name\nOne creator per line"; enabled: !dialog.frozenMetadata; clip: true; wrapMode: TextEdit.Wrap; Layout.preferredHeight: dialog.font.pixelSize * 4; Accessible.name: "Creator names, one per line" }
    TextField { id: license; objectName: "zenodoLicense"; Kirigami.FormData.label: "&License:"; text: "cc-by-4.0"; enabled: !dialog.frozenMetadata; Accessible.name: "Zenodo license identifier" }
    AppComboBox { id: access; objectName: "zenodoAccess"; Kirigami.FormData.label: "File access:"; model: ["Open after publication", "Closed files"]; enabled: !dialog.frozenMetadata; Accessible.name: "File access after publication" }
    AppComboBox { id: service; objectName: "zenodoService"; Kirigami.FormData.label: "Service:"; model: ["Sandbox (test service)", "Zenodo (production)"]; enabled: !dialog.attempted; Accessible.name: "Zenodo service" }
    Label { Kirigami.FormData.isSection: true; text: "Dataset metadata becomes public under CC0 when published, including for closed files. Review the license and access choice before publication."; wrapMode: Text.Wrap; Layout.preferredWidth: dialog.font.pixelSize * 37 }
    TextField { id: token; objectName: "zenodoToken"; Kirigami.FormData.label: "&Access token:"; echoMode: TextInput.Password; enabled: !dialog.backend.busy; Accessible.name: "Runtime access token, never saved" }
    Label { Kirigami.FormData.isSection: true; text: "Token needs deposit:write for the selected service. It is cleared after each attempt and is never saved. Create tokens in your Zenodo account settings."; wrapMode: Text.Wrap; Layout.preferredWidth: dialog.font.pixelSize * 37 }
    AppButton {
        objectName: "zenodoCreateDraft"
        text: dialog.attempted ? (dialog.receipt.draft_id > 0 ? "&Retry retained draft" : "&Retry draft creation") : "&Create draft"
        enabled: dialog.ready && !dialog.backend.busy && (!dialog.attempted || dialog.canRetry)
        onClicked: dialog.start()
    }
    BusyIndicator { running: dialog.backend.busy; visible: running; Layout.preferredWidth: dialog.font.pixelSize * 2; Layout.preferredHeight: Layout.preferredWidth }
    Label { visible: dialog.backend.busy; text: dialog.backend.message; wrapMode: Text.Wrap; Layout.preferredWidth: dialog.font.pixelSize * 37; Accessible.name: "Submission stage" }
    AppButton { text: "Continue in &background"; visible: dialog.backend.busy; onClicked: dialog.close() }
    AppErrorLabel { text: dialog.error; visible: text.length > 0; wrapMode: Text.Wrap; Layout.preferredWidth: dialog.font.pixelSize * 37 }
    TextArea {
        objectName: "zenodoResult"
        visible: !!dialog.receipt.status
        text: (dialog.receipt.message || "") + (dialog.receipt.url ? "\n" + dialog.receipt.url : "")
            + (dialog.resumePath ? "\nRecovery receipt: " + dialog.resumePath : "")
        readOnly: true; selectByMouse: true; wrapMode: TextEdit.WrapAnywhere
        Layout.preferredWidth: dialog.font.pixelSize * 37
        Accessible.name: "Draft status, link and recovery receipt"
    }
    AppButton {
        text: "Review and &publish on Zenodo"
        visible: !!dialog.receipt.url && !dialog.backend.busy
        onClicked: dialog.openExternal(dialog.receipt.url)
    }
}
