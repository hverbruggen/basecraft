import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Basecraft

AppFormDialog {
    id: dialog
    title: "BLAST at NCBI"
    standardButtons: Dialog.Close
    initialFocusItem: alphabet
    property var sourceProvider: null
    property string sourceMode: "whole"
    property var preview: ({})
    property string frozenSource: ""
    property string frozenSettings: ""
    property string formError: ""
    property string resultUrl: ""
    property string resultMessage: ""
    property bool submitted: false
    // Injectable browser boundary for tests; retry only opens the retained URL.
    property var openBrowser: url => Qt.openUrlExternally(url)
    signal copyText(string text)
    AppOperations { id: backend }
    Timer {
        interval: 200; repeat: true; running: backend.busy
        onTriggered: {
            backend.poll()
            if (!backend.busy) dialog.completed()
        }
    }
    function show(provider, mode) {
        if (backend.busy) { open(); return }
        sourceProvider = provider
        sourceMode = mode || "whole"
        sourceChoice.currentIndex = ["whole", "selection", "annotation", "all"].indexOf(sourceMode)
        preview = ({}); frozenSource = ""; frozenSettings = ""
        formError = ""; resultUrl = ""; resultMessage = ""; submitted = false
        contact.text = backend.blast_contact()
        open()
    }
    function settings() {
        return JSON.stringify({alphabet: alphabet.currentIndex === 0 ? "nucleotide" : "protein",
            database: database.currentText, expect: Number(expect.text), max_hits: maxHits.value,
            megablast: megablast.checked, contact_email: contact.text.trim()})
    }
    function invalidate() { preview = ({}); frozenSettings = ""; frozenSource = "" }
    function review() {
        formError = ""
        try {
            frozenSource = sourceProvider(sourceMode)
            frozenSettings = settings()
            preview = JSON.parse(backend.preview_blast(frozenSource, frozenSettings))
            if (preview.error) formError = preview.error
        } catch (error) { formError = String(error); invalidate() }
    }
    function start(truncate) {
        formError = ""
        try {
            const current = sourceProvider(sourceMode)
            if (current !== frozenSource || settings() !== frozenSettings) {
                invalidate()
                formError = "Source or settings changed. Review a new preview before submitting."
                return
            }
            submitted = backend.submit_blast(current, frozenSettings, preview.approval_hash, truncate)
            if (!submitted) formError = backend.message
        } catch (error) { formError = String(error) }
    }
    function openAgain() {
        if (!/^https:\/\/blast\.ncbi\.nlm\.nih\.gov\/Blast\.cgi\?CMD=Get&RID=[A-Z0-9-]{8,64}&FORMAT_TYPE=HTML$/.test(resultUrl)) return
        if (!openBrowser(resultUrl)) resultMessage = "Browser could not open. Copy the link or choose Open again."
    }
    function completed() {
        const outcome = JSON.parse(backend.outcome || "{}")
        const data = outcome.data || {}
        resultMessage = data.message || backend.message
        if (data.status === "accepted" && data.url) {
            resultUrl = data.url
            openAgain()
        }
        if (outcome.state === "partial") resultMessage += " Local evidence is incomplete; retain this link and repair publication without resubmitting."
    }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap
        text: "Selected sequences will be sent to NCBI over HTTPS. Basecraft submits once and opens your browser; NCBI runs the search. RID links are temporary."
    }
    AppComboBox {
        id: sourceChoice; Kirigami.FormData.label: "Query source"
        model: ["Whole current sequence", "Current selection", "Annotation", "All FASTA records (current unsaved first record)"]
        enabled: !backend.busy && !dialog.submitted
        onActivated: { dialog.sourceMode = ["whole", "selection", "annotation", "all"][currentIndex]; dialog.invalidate() }
    }
    AppComboBox {
        id: alphabet; Kirigami.FormData.label: "Query alphabet"
        model: ["Nucleotide", "Protein"]
        enabled: !backend.busy && !dialog.submitted
        onActivated: dialog.invalidate()
    }
    AppComboBox {
        id: database; Kirigami.FormData.label: "Target database"
        model: ["nt", "nr"]; enabled: !backend.busy && !dialog.submitted
        onActivated: dialog.invalidate()
    }
    Label {
        Kirigami.FormData.label: "Program"
        text: alphabet.currentIndex === 0 ? (database.currentText === "nt" ? "blastn" : "blastx") : (database.currentText === "nr" ? "blastp" : "tblastn")
    }
    TextField {
        id: contact; Kirigami.FormData.label: "NCBI contact email"
        enabled: !backend.busy && !dialog.submitted
        placeholderText: "Authorized service contact"
        onTextEdited: dialog.invalidate()
    }
    TextField {
        id: expect; Kirigami.FormData.label: "E-value"
        text: "0.05"; enabled: !backend.busy && !dialog.submitted
        onTextEdited: dialog.invalidate()
    }
    SpinBox {
        id: maxHits; Kirigami.FormData.label: "Maximum hits"
        from: 1; to: 5000; value: 100; editable: true
        enabled: !backend.busy && !dialog.submitted
        onValueModified: dialog.invalidate()
    }
    CheckBox {
        id: megablast; Kirigami.FormData.label: "Nucleotide search"
        text: "Megablast (close matches)"
        enabled: alphabet.currentIndex === 0 && database.currentText === "nt" && !backend.busy && !dialog.submitted
        onToggled: dialog.invalidate()
    }
    AppButton {
        text: "Preview queries"; enabled: !backend.busy && !dialog.submitted
        onClicked: dialog.review()
    }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap; visible: !!dialog.preview.approval_hash
        text: (dialog.preview.records || []).length + " records, " + dialog.preview.original_residues + " residues. Service limit: " + dialog.preview.limit + ". Proposed submission: " + dialog.preview.submitted_residues + " residues."
    }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap; visible: dialog.preview.oversized === true
        text: "Oversized query. Proposed policy: keep leading residues in input order, retain complete records until one must be shortened, then omit subsequent records. Cancel/Close sends nothing; change the source and preview again, or explicitly proceed with truncation. The original source stays unchanged."
    }
    Repeater {
        model: dialog.preview.records || []
        delegate: Label {
            required property var modelData
            Layout.fillWidth: true; wrapMode: Text.Wrap
            text: modelData.id + " “" + modelData.name + "”: " + (modelData.omitted ? "OMITTED (" + modelData.original_length + " residues)" : "keep query positions 1–" + modelData.submitted_length + " of " + modelData.original_length + (modelData.shortened ? " — SHORTENED" : "")) + "; source " + JSON.stringify(modelData.source) + (modelData.removed_gaps_or_whitespace ? "; removed " + modelData.removed_gaps_or_whitespace + " gap/whitespace characters" : "")
        }
    }
    AppButton {
        text: dialog.preview.oversized ? "Proceed with truncation and open browser" : "Submit and open browser"
        enabled: !!dialog.preview.approval_hash && !dialog.preview.error && !backend.busy && !dialog.submitted
        onClicked: dialog.start(dialog.preview.oversized === true)
    }
    AppButton { text: "Cancel submission"; visible: backend.busy; onClicked: backend.cancel() }
    Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: dialog.formError; visible: text.length > 0; Accessible.role: Accessible.AlertMessage }
    Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: backend.busy ? "Submitting to NCBI. Cancellation after upload cannot undo the remote search." : dialog.resultMessage; visible: text.length > 0 }
    TextField { Layout.fillWidth: true; readOnly: true; visible: dialog.resultUrl.length > 0; text: dialog.resultUrl; Accessible.name: "Accepted BLAST result link"; selectByMouse: true }
    RowLayout {
        visible: dialog.resultUrl.length > 0
        AppButton { text: "Copy link"; onClicked: dialog.copyText(dialog.resultUrl) }
        AppButton { text: "Open again"; onClicked: dialog.openAgain() }
    }
}
