import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

AppFormDialog {
    id: dialog
    title: "BLAST at NCBI"
    standardButtons: Dialog.Close
    initialFocusItem: program
    property var sourceProvider: null
    property string sourceMode: "whole"
    property bool alignment: false
    readonly property var modes: alignment ? ["all", "rows", "selection"] : ["whole", "selection", "annotation", "all"]
    property var preview: ({})
    property string frozenSource: ""
    property string frozenSettings: ""
    property string formError: ""
    property string resultUrl: ""
    property string resultMessage: ""
    property string publicationWarning: ""
    property bool submitted: false
    property bool browserPending: false
    // Injectable browser boundary for tests; retry only opens the retained URL.
    property var openBrowser: url => backend.open_browser_window(url)
    property var openSystemBrowser: url => Qt.openUrlExternally(url)
    property string configuredContact: ""
    readonly property var programAlphabets: ["nucleotide", "nucleotide", "protein", "protein"]
    readonly property var programDatabases: ["nt", "nr", "nr", "nt"]
    signal copyText(string text)
    required property var backend
    Connections {
        target: dialog.backend
        function onBusyChanged() { if (!dialog.backend.busy) dialog.completed() }
    }
    function show(provider, mode, isAlignment) {
        if (backend.busy) { open(); return }
        alignment = isAlignment === true
        sourceProvider = provider
        sourceMode = mode || "whole"
        sourceChoice.currentIndex = modes.indexOf(sourceMode)
        preview = ({}); frozenSource = ""; frozenSettings = ""
        formError = ""; resultUrl = ""; resultMessage = ""; publicationWarning = ""; submitted = false
        configuredContact = backend.blast_contact()
        contact.text = configuredContact
        options.checked = false
        chooseDefault()
        open()
    }
    function chooseDefault() {
        try {
            program.currentIndex = backend.blast_default_alphabet(sourceProvider(sourceMode)) === "protein" ? 2 : 0
            megablast.checked = false
        } catch (error) { formError = String(error) }
    }
    function settings() {
        return JSON.stringify({alphabet: programAlphabets[program.currentIndex],
            database: programDatabases[program.currentIndex], expect: Number(expect.text), max_hits: maxHits.value,
            megablast: program.currentIndex === 0 && megablast.checked, contact_email: contact.text.trim()})
    }
    // Search prepares and validates locally. Oversized queries stop here for
    // a separate explicit truncation decision; ordinary queries submit once.
    function search() {
        if (backend.busy || submitted) return
        review()
        if (preview.approval_hash && !preview.error && !preview.oversized) start(false)
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
        if (backend.busy || submitted) return
        formError = ""
        try {
            const current = sourceProvider(sourceMode)
            if (current !== frozenSource || settings() !== frozenSettings) {
                invalidate()
                formError = "Source or settings changed. Search again before submitting."
                return
            }
            submitted = backend.submit_blast(current, frozenSettings, preview.approval_hash, truncate)
            if (!submitted) formError = backend.message
        } catch (error) { formError = String(error) }
    }
    Connections {
        target: dialog.backend
        function onBrowserFinished(url, status) {
            dialog.browserPending = false
            if (url === dialog.resultUrl) dialog.browserResult(status)
        }
    }
    function browserResult(opened) {
        if (opened === "pending" || opened === "busy") { browserPending = true; return }
        if (opened === "unsupported") {
            resultMessage = openSystemBrowser(resultUrl)
                ? "Opened in the system browser. This browser/platform controls whether a new window or tab is used."
                : "Browser could not open. Copy the link or choose Open again."
        } else if (opened !== "opened") resultMessage = "Browser could not open. Copy the link or choose Open again."
        if (publicationWarning && !resultMessage.includes(publicationWarning)) resultMessage += publicationWarning
    }
    function openAgain() {
        if (browserPending) return
        if (!/^https:\/\/blast\.ncbi\.nlm\.nih\.gov\/Blast\.cgi\?CMD=Get&RID=[A-Z0-9-]{8,64}&FORMAT_TYPE=HTML$/.test(resultUrl)) return
        browserResult(openBrowser(resultUrl))
    }

    function completed() {
        const outcome = JSON.parse(backend.outcome || "{}")
        const data = outcome.data || {}
        publicationWarning = outcome.state === "partial" ? " Local evidence is incomplete; retain this link and repair publication without resubmitting." : ""
        resultMessage = (data.message || backend.message) + publicationWarning
        if (data.status === "accepted" && data.url) {
            resultUrl = data.url
            openAgain()
        }
    }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap
        text: "Search sends the chosen sequence to NCBI and opens browser results. RID links are temporary."
    }
    AppComboBox {
        id: sourceChoice; objectName: "blastSource"; Kirigami.FormData.label: "Query source"
        model: dialog.alignment ? ["All current FASTA rows", "Selected rows (whole sequences)", "Selected rows and columns"] : ["Whole current sequence", "Current selection", "Annotation", "All FASTA records (current unsaved first record)"]
        enabled: !backend.busy && !dialog.submitted
        onActivated: { dialog.sourceMode = dialog.modes[currentIndex]; dialog.invalidate(); dialog.chooseDefault() }
    }
    AppComboBox {
        id: program; objectName: "blastProgram"; Kirigami.FormData.label: "Program"
        model: ["blastn — nucleotide → nucleotide (nt)", "blastx — translated nucleotide → protein (nr)",
            "blastp — protein → protein (nr)", "tblastn — protein → translated nucleotide (nt)"]
        enabled: !backend.busy && !dialog.submitted
        onActivated: { megablast.checked = false; dialog.invalidate() }
    }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap
        text: "Program defaults from the input molecule or letters. Check ambiguous sequences before searching."
    }
    TextField {
        id: contact; objectName: "blastContact"; Kirigami.FormData.label: "NCBI contact email"
        enabled: !backend.busy && !dialog.submitted
        visible: !dialog.configuredContact.length || options.checked
        placeholderText: "NCBI API contact (required)"
        onTextEdited: dialog.invalidate()
    }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap; visible: contact.visible
        text: "NCBI asks API clients to include a contact email for service problems."
    }
    CheckBox { id: options; text: "Search options"; enabled: !backend.busy && !dialog.submitted }
    TextField {
        visible: options.checked
        id: expect; Kirigami.FormData.label: "E-value"
        text: "0.05"; enabled: !backend.busy && !dialog.submitted
        onTextEdited: dialog.invalidate()
    }
    AppSpinBox {
        visible: options.checked
        id: maxHits; Kirigami.FormData.label: "Maximum hits"
        from: 1; to: 5000; value: 100; editable: true
        enabled: !backend.busy && !dialog.submitted
        onValueModified: dialog.invalidate()
    }
    CheckBox {
        visible: options.checked && program.currentIndex === 0
        id: megablast; Kirigami.FormData.label: "Nucleotide search"
        text: "Megablast (close matches)"
        enabled: program.currentIndex === 0 && !backend.busy && !dialog.submitted
        onToggled: dialog.invalidate()
    }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap; visible: dialog.preview.oversized === true
        text: (dialog.preview.records || []).length + " records, " + dialog.preview.original_residues + " residues. Service limit: " + dialog.preview.limit + ". Proposed submission: " + dialog.preview.submitted_residues + " residues."
    }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap; visible: dialog.preview.oversized === true
        text: "Oversized query. Proposed policy: keep leading residues in input order, retain complete records until one must be shortened, then omit subsequent records. Cancel/Close sends nothing; change the source and search again, or explicitly proceed with truncation. The original source stays unchanged."
    }
    ColumnLayout {
      visible: dialog.preview.oversized === true
      Layout.fillWidth: true
      Repeater {
        model: dialog.preview.records || []
        delegate: Label {
            required property var modelData
            Layout.fillWidth: true; wrapMode: Text.Wrap
            text: modelData.id + " “" + modelData.name + "”: " + (modelData.omitted ? "OMITTED (" + modelData.original_length + " residues)" : "keep query positions 1–" + modelData.submitted_length + " of " + modelData.original_length + (modelData.shortened ? " — SHORTENED" : "")) + "; submitted source ranges " + JSON.stringify(modelData.submitted_source_ranges || []) + "; source " + JSON.stringify(modelData.source) + (modelData.removed_gaps_or_whitespace ? "; removed " + modelData.removed_gaps_or_whitespace + " gap/whitespace characters" : "")
        }
    }
    }
    AppButton {
        objectName: "blastSearch"
        text: dialog.preview.oversized ? "Submit truncated query" : "Search"
        enabled: !backend.busy && !dialog.submitted
        onClicked: { if (dialog.preview.oversized && dialog.preview.approval_hash) dialog.start(true); else dialog.search() }
    }
    Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: dialog.formError; visible: text.length > 0; Accessible.role: Accessible.AlertMessage }
    AppMessageBar {
        Layout.fillWidth: true
        busy: backend.busy
        workingText: "Submitting to NCBI"
        message: dialog.resultMessage
        cancelText: "Cancel submission"
        onCancelRequested: backend.cancel()
    }
    Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: "Cancellation after upload cannot undo the remote search."; visible: backend.busy }
    TextField { Layout.fillWidth: true; readOnly: true; visible: dialog.resultUrl.length > 0; text: dialog.resultUrl; Accessible.name: "Accepted BLAST result link"; selectByMouse: true }
    RowLayout {
        visible: dialog.resultUrl.length > 0
        AppButton { text: "Copy link"; onClicked: dialog.copyText(dialog.resultUrl) }
        AppButton { text: "Open again"; onClicked: dialog.openAgain() }
    }
}
