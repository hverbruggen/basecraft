import QtCore
import QtQuick
import QtQuick.Controls.Basic
import org.kde.kirigami as Kirigami
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Window
import Qt.labs.qmlmodels
import Basecraft

ApplicationWindow {
    id: window
    width: 1200
    height: 760
    visible: true
    title: current ? fileName(currentPath) + " — Basecraft" : "Basecraft"
    color: shell.background
    background: Rectangle { color: window.color }
    // A compact default with a persistent, point-based accessibility override.
    font.family: shell.mono_font
    readonly property int defaultFontSizePoints: Math.max(6, Math.round(Qt.application.font.pointSize > 0 ? Qt.application.font.pointSize : 12) - 3)
    font.pointSize: preferenceValues.ui && preferenceValues.ui.font_size_points !== null && preferenceValues.ui.font_size_points !== undefined
        ? preferenceValues.ui.font_size_points : defaultFontSizePoints

    // Closing the window (or Ctrl+Q) asks about unsaved changes first.
    property bool closeConfirmed: false
    property bool discardAuthoredOnQuit: false
    onClosing: (close) => {
        if (closeConfirmed) return
        if (!discardAuthoredOnQuit && documentRecord.unsavedDrafts().length) {
            close.accepted = false
            draftQuitDialog.open()
            return
        }
        const unsaved = window.unsavedTabs()
        if (unsaved.length === 0) return
        close.accepted = false
        quitDialog.ask(unsaved)
    }
    function unsavedTabs() {
        const out = []
        for (let i = 0; i < openFiles.count; i++) {
            const d = docs.itemAt(i)
            if (d && d.controller.dirty) out.push(i)
        }
        return out
    }

    palette.window: shell.background
    palette.base: Qt.lighter(shell.background, 1.04)
    palette.button: Qt.darker(shell.background, 1.05)
    palette.text: shell.foreground
    palette.windowText: shell.foreground
    palette.buttonText: shell.foreground
    palette.highlight: shell.accent
    palette.highlightedText: shell.background

    // Theme, font and project folder, whether or not a file is open.
    GenomeController { id: shell }

    // Open files, one tab each.
    ListModel { id: openFiles }

    // The active tab's controller and view, or null.
    property var current: null
    property var currentView: null
    readonly property var traceDocument: traceOpen ? docs.itemAt(tabs.currentIndex) : null
    readonly property var alignmentDocument: alignmentOpen ? docs.itemAt(tabs.currentIndex) : null
    property string currentPath: ""
    // "sequence" or "alignment", for the active tab.
    property string currentKind: ""
    property bool sidebarOpen: true
    property bool showGc: true
    // The chosen ribbon tab, by name.
    property string ribbonTabName: "File"
    property string defaultLayout: "wrapped"
    property int orfMinCodons: 100
    // Typing in the alignment editor inserts bases (true) or types over
    // them (false). Shared by all alignment tabs.
    property bool alignmentInsert: true
    readonly property bool alignmentOpen: currentKind === "alignment"
    readonly property bool treeOpen: currentKind === "tree"
    readonly property bool delimitationOpen: currentKind === "delimitation"
    readonly property bool traceOpen: currentKind === "trace"
    // The third tab is Annotations for a sequence, Alignment for an
    // alignment, Tree for a tree; an alignment also gets Build phylogeny.
    readonly property var ribbonTabs: ["File", "View", alignmentOpen ? "Alignment" : treeOpen ? "Tree" : delimitationOpen ? "Delimitation" : traceOpen ? "Trace" : "Annotations"]
        .concat(alignmentOpen ? ["Build phylogeny"] : [], ["Settings", "Help"])
    // The chosen tab if this document has it, else its third tab (a tab
    // for another kind of document), else File.
    readonly property string ribbonTab: ribbonTabs.includes(ribbonTabName) ? ribbonTabName
        : ["Annotations", "Alignment", "Tree", "Build phylogeny", "Delimitation", "Trace"].includes(ribbonTabName) ? ribbonTabs[2] : "File"
    // Each tab's panel in the ribbon's stack.
    readonly property var ribbonPanels: ({ "File": 0, "View": 1, "Annotations": 2, "Settings": 3, "Help": 4, "Alignment": 5, "Tree": 6, "Build phylogeny": 7, "Delimitation": 8, "Trace": 9 })
    // Only the wrapped layout is offered for now; files always open in it.

    readonly property int availableCpuThreads: shell.available_cpu_threads()
    property int localCpuThreads: Math.min(4, availableCpuThreads)
    property bool computeReady: false
    onLocalCpuThreadsChanged: {
        if (computeReady) shell.configure_cpu_threads(localCpuThreads)
        persistPreference("localCpuThreads", localCpuThreads)
    }
    onShowGcChanged: persistPreference("showGc", showGc)
    onSidebarOpenChanged: persistPreference("sidebarOpen", sidebarOpen)
    onOrfMinCodonsChanged: persistPreference("orfMinCodons", orfMinCodons)
    onAlignmentInsertChanged: persistPreference("alignmentInsert", alignmentInsert)
    readonly property var preferenceValues: preferences.snapshot.length ? JSON.parse(preferences.snapshot) : ({})
    readonly property var customization: preferenceValues.customization || ({ shortcuts: {}, ribbon_hidden: [] })
    property var ribbonCommands: []
    property var pendingRibbonCommands: []
    function publishRibbonCommands() {
        ribbonCommands = pendingRibbonCommands.slice()
    }
    function registerRibbonCommand(command) {
        if (!pendingRibbonCommands.some(item => item.commandId === command.commandId)) {
            pendingRibbonCommands.push(command)
            Qt.callLater(publishRibbonCommands)
        }
    }
    readonly property var applicationCommands: [openCommand, saveCommand, saveAsCommand, projectCommand, shortcutsCommand, aboutCommand, preferencesCommand, textRepresentationCommand, contextShortcutsCommand, historyCommand, methodsCommand, showProvenanceCommand, verifyProvenanceCommand, packageCommand, recoverCommand].concat(ribbonCommands)
    property var recentProjects: []
    property string recentProjectsError: ""
    function updateRecentProjects(reply) {
        const result = JSON.parse(reply)
        recentProjects = result.projects || []
        recentProjectsError = result.error || ""
    }
    function selectProject(path) {
        if (!preferences.project_is_directory(path)) {
            recentProjectsError = "Project folder is unavailable: " + path
            return
        }
        shell.project_dir = path
        const result = JSON.parse(preferences.remember_project(path))
        if (result.error) {
            recentProjectsError = result.error
            return
        }
        recentProjects = result.projects
        recentProjectsError = ""
    }
    property bool preferencesReady: false
    AppSettings {
        id: preferences
        onSnapshotChanged: {
            if (!window.preferencesReady) return
            const prefs = JSON.parse(snapshot)
            window.preferencesReady = false
            window.showGc = prefs.ui.show_gc
            window.sidebarOpen = prefs.ui.sidebar_open
            window.orfMinCodons = prefs.ui.orf_min_codons
            window.alignmentInsert = prefs.ui.alignment_insert
            window.localCpuThreads = Math.min(window.availableCpuThreads, prefs.compute.local_cpu_threads)
            window.preferencesReady = true
        }
        onErrorChanged: if (error.length && window.preferencesReady) {
            preferencesError.open()
        }
    }
    // Read the original QSettings store only for one-time import. No aliases
    // or assignments write back into it.
    Settings { id: legacyPreferences; category: "ui" }
    function persistPreference(key, value) {
        if (preferencesReady) preferences.update(key, JSON.stringify(value))
    }
    AppContentDialog {
        id: preferencesError
        title: "Preferences could not be loaded or saved"
        modal: true
        standardButtons: Dialog.Close
        body: ColumnLayout {
            AppErrorLabel {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: preferences.error + "\n\nThe original preferences are retained. If loading failed, changes in this session remain temporary. Correct the preferences file or storage problem, then retry."
            }
            AppButton {
                text: "Retry loading preferences"
                onClicked: {
                    window.loadPreferences()
                    if (!preferences.error.length) preferencesError.close()
                }
            }
        }
    }
    function loadPreferences() {
        preferencesReady = false
        if (!preferences.reload()) return
        const legacy = {}
        for (const key of ["showGc", "sidebarOpen", "orfMinCodons", "alignmentInsert", "localCpuThreads"])
            legacy[key] = legacyPreferences.value(key, null)
        if (!preferences.import_legacy(JSON.stringify(legacy))) return
        const prefs = JSON.parse(preferences.snapshot)
        showGc = prefs.ui.show_gc
        sidebarOpen = prefs.ui.sidebar_open
        orfMinCodons = prefs.ui.orf_min_codons
        alignmentInsert = prefs.ui.alignment_insert
        localCpuThreads = Math.max(1, Math.min(availableCpuThreads, prefs.compute.local_cpu_threads))
        preferencesReady = true
    }

    function fileName(path) { return path.split("/").pop() }
    function activeFolder() {
        const path = traceOpen && current && current.export_path ? current.export_path : current && current.file_path ? current.file_path : currentPath
        return path.length ? shell.parent_folder_url(path) : shell.local_file_url(shell.project_dir)
    }

    function showActivity() { tasksDialog.open() }
    function localPath(url) { return shell.local_file_path(url) }

    property bool focusOpenedDocument: false

    function refreshCurrent() {
        const d = docs.itemAt(tabs.currentIndex)
        current = d ? d.controller : null
        currentView = d ? d.view : null
        currentPath = d ? d.path : ""
        const firstOpen = d && d.kind === "alignment" && !d.ribbonInitialized
        currentKind = d ? d.kind : ""
        if (firstOpen) { d.ribbonInitialized = true; if (currentKind === "alignment") ribbonTabName = "Alignment" }
        if (currentView && (focusOpenedDocument || !tabs.activeFocus)) {
            // Background model updates must not steal focus from controls/dialogs.
            if (focusOpenedDocument || !window.activeFocusItem || !window.activeFocusItem.visible)
                currentView.forceActiveFocus(Qt.OtherFocusReason)
        }
        if (currentView) focusOpenedDocument = false
    }

    function openTextRepresentation() {
        if (!currentView) return
        const source = current
        if (!source || typeof source.accessible_page !== "function") return
        textRepresentation.source = source
        textRepresentation.open()
    }
    TextRepresentationDialog { id: textRepresentation }

    function focusRegion(backward) {
        const regions = [ribbonTabBar, tree, tabs, delimitationOpen ? docs.itemAt(tabs.currentIndex) : currentView].filter(item => item && item.visible && item.enabled)
        if (!regions.length) return
        let active = -1
        for (let i = 0; i < regions.length; ++i) {
            for (let item = window.activeFocusItem; item; item = item.parent) {
                if (item === regions[i]) { active = i; break }
            }
        }
        if (active < 0) return
        const region = regions[(active + (backward ? regions.length - 1 : 1)) % regions.length]
        const target = region === ribbonTabBar || region === tabs ? region.currentItem : region
        if (target) target.forceActiveFocus(Qt.TabFocusReason)
    }
    Shortcut { sequence: "F6"; onActivated: window.focusRegion(false) }
    Shortcut { sequence: "Shift+F6"; onActivated: window.focusRegion(true) }

    // Open a file in a tab, or switch to its tab if it's already open.
    function openPath(path) {
        focusOpenedDocument = true
        for (let i = 0; i < openFiles.count; i++) {
            if (openFiles.get(i).path === path) { tabs.currentIndex = i; Qt.callLater(refreshCurrent); return }
        }
        if (!shell.project_dir.length) shell.use_project_of(path)
        openFiles.append({ path: path, dirty: false, kind: shell.file_kind(path) })
        tabs.currentIndex = openFiles.count - 1
    }

    property var delimitationSource: null
    function beginDelimitation(method) {
        if (!current || current.busy) return
        delimitationSource = current
        speciesDialog.show(method, alignmentOpen ? current.file_path : current.linked_alignment || "",
            treeOpen ? current.file_path : delimitationOpen ? current.display_path : "")
    }
    DelimitationAnalysisDialog {
        id: speciesDialog
        pathResolver: shell
        workers: window.localCpuThreads
        onRequested: (method, input, rooted, settings, displayMethod) => {
            const source = window.delimitationSource
            if (!source) return
            if (window.delimitationOpen && source === window.current) {
                docs.itemAt(tabs.currentIndex).startAnalysis(method, input, rooted, settings, displayMethod)
                return
            }
            const usesOpenAlignment = window.alignmentOpen && input === source.file_path
            const path = source.create_delimitation()
            if (!path.length) return
            window.openPath(path)
            const index = tabs.currentIndex
            Qt.callLater(() => {
                const document = docs.itemAt(index)
                if (document && document.kind === "delimitation") {
                    window.ribbonTabName = "Delimitation"
                    document.startAnalysis(method, usesOpenAlignment ? document.controller.linked_alignment : input, rooted, settings, displayMethod)
                }
            })
        }
    }
    DelimitationController { id: combineController }
    FileDialog {
        id: combinePicker
        title: "Combine saved species hypotheses"
        fileMode: FileDialog.OpenFiles
        nameFilters: ["Species hypotheses (*.spart *.spartxml *.spart-xml *.xml)"]
        onAccepted: {
            if (selectedFiles.length < 2) { combineError.message = "Choose at least two species hypothesis files."; combineError.open(); return }
            combinedDestination.open()
        }
    }
    FileDialog {
        id: combinedDestination
        title: "Save combined species hypotheses as a new file"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "spart"
        nameFilters: ["Species hypotheses (*.spart)"]
        onAccepted: {
            const paths = Array.from(combinePicker.selectedFiles, url => window.localPath(url))
            const output = window.localPath(selectedFile)
            if (combineController.combine_files(JSON.stringify(paths), output)) window.openPath(output)
            else { combineError.message = combineController.message; combineError.open() }
        }
    }
    AppContentDialog {
        id: combineError
        property string message: ""
        title: "Could not combine hypotheses"
        standardButtons: Dialog.Close
        body: AppErrorLabel { text: combineError.message; wrapMode: Text.Wrap }
    }

    function startTraceReview(paths, reference, target, cutoff = 20, forceNew = false) {
        let preserve = forceNew
        for (let i = 0; i < openFiles.count; i++) {
            const existing = docs.itemAt(i)
            if (openFiles.get(i).path === paths[0] && existing && existing.controller.dirty) preserve = true
        }
        if (preserve) {
            openFiles.append({ path: paths[0], dirty: false, kind: "trace" })
            tabs.currentIndex = openFiles.count - 1
        } else window.openPath(paths[0])
        const traceIndex = tabs.currentIndex
        Qt.callLater(() => { ribbonTabName = "Trace"; docs.itemAt(traceIndex).startReview(paths, reference, target, cutoff) })
    }

    Connections {
        target: window.current
        ignoreUnknownSignals: true
        function onTaxaSelected(names) {
            if (!window.current) return
            const source = (window.treeOpen || window.delimitationOpen) ? window.current.linked_alignment : window.current.file_path
            if (!source) return
            for (let i = 0; i < docs.count; i++) {
                const d = docs.itemAt(i)
                if (!d || d.controller === window.current) continue
                if ((window.treeOpen || window.delimitationOpen) && d.kind === "alignment" && d.controller.file_path === source)
                    d.controller.highlight_taxa(names)
                else if (window.alignmentOpen && (d.kind === "tree" || d.kind === "delimitation") && d.controller.linked_alignment === source)
                    d.controller.highlight_taxa(names)
            }
        }
    }

    function closeTab(i) {
        const d = docs.itemAt(i)
        if (d && d.controller.dirty) { confirmClose.index = i; confirmClose.open(); return }
        openFiles.remove(i)
        Qt.callLater(refreshCurrent)
    }

    // Clipboard access goes through a hidden text edit.
    TextEdit {
        id: clipboard
        visible: false
        function put(text) { clipboard.text = text; clipboard.selectAll(); clipboard.copy() }
    }

    AppCommand {
        id: openCommand
        customization: window.customization
        commandId: "file.open"
        text: "Open…"
        iconName: "open"
        tooltip: "Open a sequence or alignment file"
        defaultShortcut: "Ctrl+O"
        onTriggered: openDialog.open()
    }
    AppCommand {
        id: saveCommand
        customization: window.customization
        commandId: "file.save"
        text: "Save"
        iconName: "save"
        tooltip: "Save the current file or trace review"
        defaultShortcut: "Ctrl+S"
        enabled: window.current !== null && (!window.traceOpen || window.current.review_mode)
        onTriggered: window.traceOpen ? window.traceDocument.saveReview() : window.current.save()
    }
    AppCommand {
        id: saveAsCommand
        customization: window.customization
        commandId: "file.save-as"
        text: "Save as…"
        iconName: "save-as"
        tooltip: "Save as a new file and carry on in that one"
        defaultShortcut: "Ctrl+Shift+S"
        enabled: window.current !== null
        onTriggered: window.saveAs()
    }
    AppCommand {
        id: projectCommand
        customization: window.customization
        commandId: "project.open"
        text: "Open project…"
        iconName: "project"
        tooltip: "Choose or create a project folder"
        onTriggered: projectDialog.open()
    }
    AppCommand {
        id: shortcutsCommand
        customization: window.customization
        commandId: "help.shortcuts"
        text: "Keyboard shortcuts…"
        iconName: "keyboard"
        tooltip: "List all keyboard shortcuts"
        onTriggered: shortcuts.show("")
    }
    AppCommand {
        id: aboutCommand
        customization: window.customization
        commandId: "help.about"
        text: "About Basecraft…"
        iconName: "info"
        tooltip: "Version and licence"
        onTriggered: about.open()
    }

    AppCommand {
        id: preferencesCommand
        customization: window.customization
        commandId: "application.preferences"
        text: "Preferences…"
        iconName: "info"
        tooltip: "Appearance, files, compute, ribbon and shortcuts"
        onTriggered: preferencesDialog.open()
    }
    AppCommand {
        id: textRepresentationCommand
        customization: window.customization
        commandId: "view.text"
        text: "Text representation…"
        iconName: "text-representation"
        tooltip: "Read and copy the current scientific document as text"
        defaultShortcut: "Ctrl+Alt+T"
        enabled: window.current !== null
        onTriggered: window.openTextRepresentation()
    }
    AppCommand {
        id: contextShortcutsCommand
        customization: window.customization
        commandId: "help.context-shortcuts"
        text: "Shortcuts…"
        iconName: "keyboard"
        tooltip: "Keyboard shortcuts for this tab"
        onTriggered: shortcuts.show(window.ribbonTab)
    }
    AppProvenance {
        id: provenance
        onMessageChanged: if (message.length && message !== "notify: Working…") notification("", message, message.startsWith("fail:") ? "fail" : "done")
    }
    property var activeTasks: ({jobs: []})
    readonly property int runningTaskCount: (activeTasks.jobs || []).filter(job => job.status === "running").length
    readonly property int failedTaskCount: (activeTasks.jobs || []).filter(job => job.status === "failed").length
    Timer { interval: 1500; running: true; repeat: true; onTriggered: window.activeTasks = JSON.parse(provenance.jobs(false)) }
    property string frozenReport: ""
    property string frozenDraft: ""
    property string frozenDraftRevision: ""
    property string frozenDraftKey: ""
    DocumentRecordDialog {
        id: documentRecord
        service: provenance
        documentPath: window.currentPath
        controller: window.current
        onCopyText: text => clipboard.put(text)
        onExportReport: snapshot => { window.frozenReport = snapshot; reportFolder.open() }
        onSaveDraft: (text, revision) => { window.frozenDraft = text; window.frozenDraftRevision = revision; window.frozenDraftKey = documentRecord.draftKey; authoredFile.open() }
        onVerify: verifyProvenanceCommand.trigger()
        onPackageRecord: packageCommand.trigger()
        onOpenRecordFolder: showProvenanceCommand.trigger()
    }
    FolderDialog {
        id: reportFolder
        title: "Export the selected revision's report"
        onAccepted: { provenance.export_report(window.frozenReport, window.localPath(selectedFolder)); provenanceDialog.open() }
    }
    FileDialog {
        id: authoredFile
        title: "Save authored methods draft"
        fileMode: FileDialog.SaveFile
        nameFilters: ["Markdown (*.md)"]
        onAccepted: {
            if (provenance.save_authored(window.localPath(selectedFile), window.frozenDraft, window.frozenDraftRevision))
                documentRecord.markDraftSaved(window.frozenDraftKey, window.frozenDraft)
        }
    }
    TasksDialog {
        id: tasksDialog
        service: provenance
        onOpenDocument: path => window.openPath(path)
        onCopyText: text => clipboard.put(text)
    }
    property string provenanceSource: ""
    property string provenanceExport: ""
    function inspectProvenance(operation) {
        provenanceExport = ""
        provenanceDialog.open()
        provenance.start(operation, window.currentPath, "")
    }
    function exportProvenance(operation) {
        provenanceSource = window.currentPath
        provenanceExport = operation
        provenanceFolder.open()
    }
    AppCommand {
        id: historyCommand
        customization: window.customization
        commandId: "provenance.history"
        text: "History"
        iconName: "history"
        tooltip: "Sources, analyses and edits for the current document"
        enabled: window.current !== null && !provenance.busy
        onTriggered: documentRecord.showPage(0)
    }
    AppCommand {
        id: methodsCommand
        customization: window.customization
        commandId: "provenance.methods"
        text: "Methods"
        iconName: "text-representation"
        tooltip: "Read methods for the current document, including unsaved changes"
        enabled: window.current !== null && !provenance.busy
        onTriggered: documentRecord.showPage(1)
    }
    AppCommand {
        id: showProvenanceCommand
        customization: window.customization
        commandId: "provenance.show-files"
        text: "Record folder"
        iconName: "open"
        tooltip: "Open the saved result's readable provenance folder"
        enabled: window.current !== null && !provenance.busy
        onTriggered: {
            const folder = provenance.bundle_path(window.currentPath)
            if (folder.length) Qt.openUrlExternally(shell.local_file_url(folder))
            else provenanceDialog.open()
        }
    }
    AppCommand {
        id: verifyProvenanceCommand
        customization: window.customization
        commandId: "provenance.verify"
        text: "Verify…"
        tooltip: "Verify recorded input and output hashes without executing tools"
        enabled: window.current !== null && !provenance.busy
        onTriggered: window.inspectProvenance("verify")
    }
    AppCommand {
        id: packageCommand
        customization: window.customization
        commandId: "provenance.package"
        text: "Package for reproduction…"
        tooltip: "Copy the result and retained history to a new package directory"
        enabled: window.current !== null && !provenance.busy
        onTriggered: window.exportProvenance("package")
    }
    AppCommand {
        id: recoverCommand
        customization: window.customization
        commandId: "file.recover-work"
        text: "Recover work…"
        iconName: "open"
        tooltip: "Reconstruct saved recovery journals into a new folder"
        enabled: !provenance.busy
        onTriggered: {
            recoveryPicker.sessions = JSON.parse(provenance.recovery_sessions())
            recoveryPicker.open()
        }
    }
    AppFormDialog {
        id: recoveryPicker
        title: "Recover unsaved work"
        standardButtons: Dialog.Close
        property var sessions: []
        initialFocusItem: recoveryChoice
        Label { text: "Recovery creates copies in a new directory. It preserves your originals and the recovery journal."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { visible: !recoveryPicker.sessions.length; text: provenance.message.startsWith("fail:") ? provenance.message : "No recovery sessions are available."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        AppComboBox {
            id: recoveryChoice
            Kirigami.FormData.label: "Session:"
            Layout.fillWidth: true
            model: recoveryPicker.sessions.map(session => session.name + " — " + session.id)
            enabled: model.length > 0
        }
        AppButton {
            text: "Choose recovery destination…"
            enabled: recoveryChoice.currentIndex >= 0 && recoveryPicker.sessions.length > 0
            onClicked: {
                window.provenanceSource = recoveryPicker.sessions[recoveryChoice.currentIndex].path
                window.provenanceExport = "recover"
                recoveryPicker.close()
                provenanceFolder.open()
            }
        }
    }
    FolderDialog {
        id: provenanceFolder
        title: window.provenanceExport === "methods" ? "Choose the folder for generated reports" : window.provenanceExport === "recover" ? "Choose the parent folder for recovered copies" : "Choose the parent folder for a new reproduction package"
        onAccepted: {
            let destination = window.localPath(selectedFolder)
            if (window.provenanceExport === "package") destination += "/" + window.fileName(window.provenanceSource) + ".reproduction"
            if (window.provenanceExport === "recover") destination += "/" + window.fileName(window.provenanceSource) + ".recovered"
            provenanceDialog.open()
            provenance.start(window.provenanceExport, window.provenanceSource, destination)
        }
    }
    AppDialog {
        id: provenanceDialog
        title: window.provenanceExport === "recover" ? "Recovered work" : "Saved files and exports"
        implicitWidth: 860
        implicitHeight: 600
        standardButtons: Dialog.Close
        initialFocusItem: provenanceText
        contentItem: ColumnLayout {
            Label { visible: window.provenanceExport !== "recover"; text: "File verification and reproduction packages describe saved files. Report exports retain the selected document revision."; wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label { text: provenance.message; wrapMode: Text.Wrap; Layout.fillWidth: true; Accessible.role: Accessible.AlertMessage }
            BusyIndicator { running: provenance.busy; visible: running }
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                TextArea { id: provenanceText; text: provenance.text; readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap; Accessible.name: "Recorded provenance and verification results" }
            }
        }
    }
    PreferencesDialog { id: preferencesDialog; settings: preferences; commands: window.applicationCommands; defaultFontSizePoints: window.defaultFontSizePoints }

    function ribbonIconFor(id) {
        if (!id) return "info"
        if (id.includes("copy-") || id.includes("extract-")) return "selection"
        if (id.includes("export-") || id.includes("save-") || id.includes("package-")) return "save-as"
        if (id.includes("annotation") || id.includes("annotate-")) return "annotation-add"
        if (id.includes("rename-") || id.includes("edit-")) return "edit"
        if (id.includes("link-") || id.includes("associate-")) return "link"
        if (id.includes("order-") || id.includes("above-")) return "up"
        if (id.includes("below-")) return "down"
        if (id.includes("flip-") || id.includes("reverse-")) return "arrow-right"
        if (id.includes("tree") || id.includes("phylogeny") || id.includes("clade") || id.includes("distance.")) return "tree"
        if (id.includes("mafft") || id.includes("trimal") || id.includes("alignment") || id.includes("matrix")) return "columns"
        if (id.includes("trace") || id.includes("draft") || id.includes("signal") || id.includes("primer") || id.includes("consensus")) return "waveform"
        if (id.includes("find-") || id.includes("predict-")) return "bug"
        if (id.includes("open-")) return "open"
        if (id.includes("move-")) return "arrow-right"
        if (id.includes("add-")) return "add"
        return "info"
    }

    component RibbonButton: AppCommandButton {
        customization: window.customization
        enabled: window.current !== null
        iconName: window.ribbonIconFor(commandId)
        Component.onCompleted: window.registerRibbonCommand(commandAction)
    }
    // The same last group on every ribbon tab.
    // A ribbon button that shows whether its mode is on.
    component ExportConsensusButton: RibbonButton {
        id: consensusExportButton
        background: Rectangle {
            radius: 3
            color: Qt.alpha(shell.foreground, consensusExportButton.pressed ? 0.14 : consensusExportButton.hovered ? 0.10 : 0.05)
            border.color: Qt.alpha(shell.foreground, 0.18)
        }
    }
    component ModeButton: RibbonButton {
        id: modeButton
        property bool active
        background: Rectangle {
            radius: 3
            color: modeButton.active ? Qt.alpha(shell.accent, 0.18)
                : modeButton.hovered ? Qt.alpha(shell.foreground, 0.05) : "transparent"
            border.color: modeButton.active || modeButton.visualFocus ? shell.accent : "transparent"
        }
    }
    component SpeciesMethodsGroup: RibbonGroup {
        id: speciesMethods
        required property string commandSuffix
        title: "Species delimitation"
        RibbonButton { commandId: "species.gmyc-" + speciesMethods.commandSuffix; text: "GMYC"; iconName: "delimitation"; enabled: window.current !== null && !window.current.busy; onInvoked: window.beginDelimitation("gmyc") }
        RibbonButton { commandId: "species.asap-" + speciesMethods.commandSuffix; text: "ASAP"; iconName: "delimitation"; enabled: window.current !== null && !window.current.busy; onInvoked: window.beginDelimitation("asap") }
        RibbonButton { commandId: "species.ptp-" + speciesMethods.commandSuffix; text: "PTP"; iconName: "delimitation"; enabled: window.current !== null && !window.current.busy; onInvoked: window.beginDelimitation("ptp") }
        RibbonButton { commandId: "species.manual-" + speciesMethods.commandSuffix; text: "Manual"; iconName: "tree"; enabled: window.current !== null && !window.current.busy; onInvoked: window.beginDelimitation("manual") }
        RibbonButton { commandId: "species.combine-" + speciesMethods.commandSuffix; text: "Combine"; iconName: "align"; onInvoked: combinePicker.open() }
    }
    component ShortcutsGroup: RibbonGroup {
        title: "Keys"
        actions: [contextShortcutsCommand]
    }
    header: ColumnLayout {
        spacing: 0
        RowLayout {
            visible: preferences.error.length > 0
            Layout.fillWidth: true
            Label {
                Layout.fillWidth: true
                text: "Preferences unavailable; using session values."
                wrapMode: Text.Wrap
                Accessible.role: Accessible.AlertMessage
            }
            AppButton { text: "Details…"; onClicked: preferencesError.open() }
        }
        Label {
            visible: window.recentProjectsError.length > 0
            Layout.fillWidth: true
            text: window.recentProjectsError
            wrapMode: Text.Wrap
            Accessible.role: Accessible.AlertMessage
        }

        // Ribbon tabs. Undo, redo and save are Ctrl+Z, Ctrl+Shift+Z, Ctrl+S.
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: ribbonTabRow.implicitHeight
            color: Qt.alpha(shell.foreground, 0.05)
            RowLayout {
                id: ribbonTabRow
                anchors.fill: parent
                anchors.leftMargin: 6
                spacing: 0
                RibbonTabs {
                    id: ribbonTabBar
                    Layout.fillWidth: true
                    names: window.ribbonTabs
                    selectedName: window.ribbonTab
                    onSelected: name => window.ribbonTabName = name
                }
                Label {
                    id: messageLabel
                    Layout.preferredWidth: Math.min(implicitWidth, window.width * 0.3)
                    Layout.minimumWidth: 0
                    horizontalAlignment: Text.AlignRight
                    rightPadding: 10
                    elide: Text.ElideLeft
                    text: window.current ? window.current.message : ""
                    // Green for done, red for failed, plain for hints.
                    color: !window.current ? shell.foreground
                        : window.current.message_kind === "ok" ? window.current.ok_color
                        : window.current.message_kind === "error" ? window.current.error_color
                        : shell.foreground
                    // A message goes away by itself after 10 seconds.
                    Timer {
                        id: messageTimer
                        interval: 10000
                        onTriggered: if (window.current) window.current.message = ""
                    }
                    onTextChanged: if (messageLabel.text.length > 0) messageTimer.restart()
                }
                AppButton {
                    text: window.runningTaskCount ? "Tasks (" + window.runningTaskCount + ")" : "Tasks" + (window.failedTaskCount ? " · " + window.failedTaskCount + " failed" : "")
                    Accessible.name: "Tasks and notifications, " + window.runningTaskCount + " running, " + window.failedTaskCount + " failed"
                    onClicked: tasksDialog.open()
                }
                AppIconButton {
                    iconName: "more"; description: "More"
                    focusPolicy: Qt.StrongFocus
                    ToolTip.text: "More"
                    ToolTip.visible: hovered
                    onClicked: moreMenu.popup(this, 0, height)
                    Menu {
                        id: moreMenu
                        MenuItem { text: "Tasks & notifications"; onTriggered: window.showActivity() }
                        MenuItem { action: shortcutsCommand }
                        MenuSeparator {}
                        MenuItem { action: aboutCommand }
                    }
                }
            }
        }

        Ribbon {
            id: ribbon
            objectName: "applicationRibbon"
            Layout.fillWidth: true
            currentIndex: window.ribbonPanels[window.ribbonTab]
            background: Rectangle { color: Qt.alpha(shell.foreground, 0.025) }

                RibbonPage {
                    spacing: 6
                    RibbonGroup {
                        title: "Project"
                        AppRibbonButton { action: projectCommand; visible: projectCommand.visible; text: "Open/New"; iconName: "project"; tip: projectCommand.tooltip }
                        AppRibbonButton {
                            id: recentProjectButton
                            text: "Recent"
                            iconName: "project"
                            tip: "Open a recent project"
                            Accessible.role: Accessible.ButtonMenu
                            onClicked: recentProjectMenu.popup(recentProjectButton, 0, height)
                            Menu {
                                id: recentProjectMenu
                                MenuItem { text: "No recent projects"; enabled: false; visible: window.recentProjects.length === 0 }
                                Repeater {
                                    model: window.recentProjects
                                    MenuItem {
                                        required property string modelData
                                        text: window.fileName(modelData)
                                        ToolTip.text: modelData
                                        ToolTip.visible: hovered
                                        onTriggered: window.selectProject(modelData)
                                    }
                                }
                            }
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "File"
                        AppRibbonButton { action: saveCommand; visible: saveCommand.visible; iconName: "save"; tip: saveCommand.tooltip }
                        AppRibbonButton { action: saveAsCommand; visible: saveAsCommand.visible; iconName: "save-as"; tip: saveAsCommand.tooltip }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Recovery"
                        AppRibbonButton { action: recoverCommand; visible: recoverCommand.visible; iconName: "history"; tip: recoverCommand.tooltip }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Export"
                        RibbonButton { commandId: "ribbon.export-an-editable-svg-of-this-view"; text: "Export SVG…"; tip: "Export an editable SVG of this view"; enabled: window.current !== null; onInvoked: window.exportSvg() }
                    }
                    Item { Layout.fillWidth: true }
                    RibbonGroup {
                        title: "Review files"
                        visible: window.traceOpen
                        RibbonButton { commandId: "ribbon.open-saved-draft-evidence"; text: "Open draft…"; tip: "Open saved draft evidence"; keys: "Ctrl+Shift+D"; enabled: window.current && !window.current.dirty; onInvoked: window.traceDocument.openDraft() }
                        RibbonButton { commandId: "ribbon.resume-saved-review-decisions"; text: "Open review…"; tip: "Resume saved review decisions"; keys: "Ctrl+Alt+O"; enabled: window.current && !window.current.dirty; onInvoked: window.traceDocument.openReview() }
                    }
                    RibbonGroup { title: "Document record"; actions: [historyCommand, methodsCommand] }
                    ShortcutsGroup {}
                }

                RibbonPage {
                    spacing: 6
                    RibbonGroup {
                        title: "Zoom"
                        RibbonButton { commandId: "ribbon.zoom-out"; iconName: "zoom-out"; tip: "Zoom out"; keys: "Ctrl+−"; onInvoked: (window.delimitationOpen ? window.currentView.controller : window.current).zoom_out() }
                        RibbonButton { commandId: "ribbon.zoom-in"; iconName: "zoom-in"; tip: "Zoom in"; keys: "Ctrl++"; onInvoked: (window.delimitationOpen ? window.currentView.controller : window.current).zoom_in() }
                        RibbonButton { commandId: "ribbon.fit";
                            text: "Fit"
                            iconName: "fit"
                            tip: window.alignmentOpen ? "Show the whole width of the alignment" : window.treeOpen ? "Show the whole tree" : "Show the whole sequence"
                            keys: "Ctrl+0"
                            onInvoked: (window.delimitationOpen ? window.currentView.controller : window.current).fit()
                        }
                        RibbonButton { commandId: "ribbon.zoom-to-the-selection"; text: "Selection"; iconName: "selection"; visible: commandAction.visible && (!window.treeOpen && !window.delimitationOpen && !window.traceOpen); tip: "Zoom to the selection"; onInvoked: window.current.zoom_to_selection() }
                        TextField {
                            id: zoomInput
                            objectName: "zoomPercentage"
                            readonly property var controller: window.delimitationOpen ? window.currentView.controller : window.current
                            property string currentZoom: controller ? controller.zoom_text : ""
                            FontMetrics { id: zoomMetrics; font: zoomInput.font }
                            implicitWidth: zoomMetrics.advanceWidth("100") + leftPadding + rightPadding + 8
                            enabled: controller !== null
                            Accessible.name: "Zoom percentage"
                            selectByMouse: true
                            validator: DoubleValidator { bottom: 0.000001; top: 20000; notation: DoubleValidator.StandardNotation; locale: "C" }
                            function resetText() { text = currentZoom.split("%")[0].trim() }
                            Component.onCompleted: resetText()
                            onCurrentZoomChanged: if (!activeFocus) resetText()
                            onEditingFinished: {
                                if (controller && acceptableInput) controller.set_zoom_percent(Number(text))
                                resetText()
                            }
                            Keys.onEscapePressed: { resetText(); focus = false }
                        }
                        Label { text: "%" }
                    }
                    ToolSeparator { Layout.fillHeight: true; visible: window.currentKind === "sequence" }
                    // Linear and circular layouts exist in the code but are
                    // not offered for now; the wrapped layout is the view.
                    RibbonGroup {
                        title: "Show"
                        // Only sequences have a GC plot.
                        visible: window.currentKind === "sequence"
                        CheckBox {
                            text: "GC content"
                            ToolTip.text: "Show the GC content plot"
                            ToolTip.visible: hovered
                            ToolTip.delay: 400
                            checked: window.showGc
                            focusPolicy: Qt.StrongFocus
                            onToggled: window.showGc = checked
                        }
                    }
                    Item { Layout.fillWidth: true }
                    RibbonGroup { title: "Text"; actions: [textRepresentationCommand] }
                    ShortcutsGroup {}
                }

                RibbonPage {
                    spacing: 6
                    RibbonGroup {
                        title: "Annotation"
                        RibbonButton { commandId: "ribbon.annotate-the-selection"; text: "Add…"; iconName: "annotation-add"; tip: "Annotate the selection"; keys: "Ctrl+N"; onInvoked: editor.openNew() }
                        RibbonButton { commandId: "ribbon.edit-the-selected-annotation"; text: "Edit…"; iconName: "annotation-edit"; tip: "Edit the selected annotation"; keys: "Enter"; onInvoked: editor.openSelected() }
                        RibbonButton { commandId: "ribbon.delete-the-selected-annotation";
                            text: "Delete"
                            iconName: "annotation-delete"
                            tip: "Delete the selected annotation"
                            keys: "Delete"
                            onInvoked: {
                                const id = window.current.selected_feature()
                                if (id >= 0) window.current.delete_feature(id)
                                else { window.current.message_kind = ""; window.current.message = "Click an annotation first" }
                            }
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Go to"
                        RibbonButton { commandId: "ribbon.select-the-previous-annotation"; text: "Previous"; iconName: "left"; tip: "Select the previous annotation"; onInvoked: window.current.step_feature(false) }
                        RibbonButton { commandId: "ribbon.select-the-next-annotation"; text: "Next"; iconName: "right"; tip: "Select the next annotation"; onInvoked: window.current.step_feature(true) }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Sequence"
                        RibbonButton { commandId: "ribbon.copy-the-selected-annotation-s-sequence";
                            text: "Copy"
                            tip: "Copy the selected annotation's sequence"
                            keys: "Ctrl+C"
                            onInvoked: {
                                const id = window.current.selected_feature()
                                if (id >= 0) clipboard.put(window.current.feature_sequence(id))
                                else { window.current.message_kind = ""; window.current.message = "Click an annotation first" }
                            }
                        }
                        RibbonButton { commandId: "ribbon.save-the-selection-as-a-new-file"; text: "Extract"; tip: "Save the selection as a new file"; keys: "Ctrl+E"; onInvoked: window.current.extract_selection() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Find"
                        RibbonButton { commandId: "ribbon.annotate-open-reading-frames";
                            text: "ORFs"
                            tip: "Annotate open reading frames"
                            onInvoked: orfDialog.search(false)
                        }
                        RibbonButton { commandId: "ribbon.annotate-protein-coding-genes-with-prodigal";
                            text: "Prodigal"
                            tip: "Annotate protein-coding genes with Prodigal"
                            onInvoked: genesDialog.predict(false)
                        }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RibbonPage {
                    spacing: 6
                    RibbonGroup {
                        title: "Preferences"
                        actions: [preferencesCommand]
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RibbonPage {
                    spacing: 6
                    RibbonGroup {
                        title: "Learn"
                        actions: [shortcutsCommand, aboutCommand]
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Online"
                        RibbonButton { commandId: "ribbon.open-the-project-on-github"; text: "Project page"; iconName: "link"; tip: "Open the project on GitHub"; enabled: true; onInvoked: Qt.openUrlExternally("https://github.com/hverbruggen/basecraft") }
                        RibbonButton { commandId: "ribbon.open-a-new-github-issue"; text: "Report a problem"; iconName: "bug"; tip: "Open a new GitHub issue"; enabled: true; onInvoked: Qt.openUrlExternally("https://github.com/hverbruggen/basecraft/issues/new") }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                // Alignment: edits on the selected block, its columns and rows.
                RibbonPage {
                    spacing: 6
                    RibbonGroup {
                        title: "Editing mode"
                        ModeButton { commandId: "ribbon.insert-typing-inserts-bases-the-rest-of-the-row";
                            iconName: "add"
                            text: "Insert"
                            active: window.alignmentInsert
                            tip: "Insert: typing inserts bases; the rest of the row moves right"
                            keys: "Insert"
                            onInvoked: window.alignmentInsert = true
                        }
                        ModeButton { commandId: "ribbon.overwrite-typing-replaces-the-selected-bases";
                            iconName: "edit"
                            text: "Overwrite"
                            active: !window.alignmentInsert
                            tip: "Overwrite: typing replaces the selected bases"
                            keys: "Insert"
                            onInvoked: window.alignmentInsert = false
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Columns"
                        RibbonButton { commandId: "ribbon.delete-columns-containing-only-gaps-across-all-rows-including";
                            iconName: "columns"
                            text: "Delete empty"
                            badge: window.current && window.current.gap_column_count >= 0 ? String(window.current.gap_column_count) : "…"
                            enabled: window.current !== null && window.current.gap_column_count > 0
                            tip: "Delete columns containing only gaps across all rows, including hidden rows"
                            onInvoked: window.current.remove_gap_columns()
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Rows"
                        RibbonButton { commandId: "ribbon.move-the-selected-rows-up"; text: "Up"; iconName: "up"; tip: "Move the selected rows up"; keys: "Alt+↑"; onInvoked: window.current.move_rows(true) }
                        RibbonButton { commandId: "ribbon.move-the-selected-rows-down"; text: "Down"; iconName: "down"; tip: "Move the selected rows down"; keys: "Alt+↓"; onInvoked: window.current.move_rows(false) }
                        RibbonButton { commandId: "ribbon.rename-the-cursor-s-row"; text: "Name"; tip: "Rename the cursor's row"; keys: "F2"; onInvoked: renameDialog.ask() }
                        RibbonButton { commandId: "ribbon.delete-the-selected-rows"; text: "Delete"; iconName: "delete"; tip: "Delete the selected rows"; onInvoked: window.current.remove_rows() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Align"
                        RibbonButton { commandId: "ribbon.realign-the-sequences-with-mafft-default-settings-into-a";
                            iconName: "align"
                            text: window.current && window.current.busy ? "Aligning…" : "MAFFT"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Realign the sequences with MAFFT, default settings, into a new file"
                            onInvoked: window.current.align_mafft()
                        }
                        RibbonButton { commandId: "ribbon.trim-unreliable-columns-with-trimal-automated1-into-a-new";
                            iconName: "trim"
                            text: "trimAL"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Trim unreliable columns with trimAl automated1 into a new alignment"
                            onInvoked: window.current.trim_alignment()
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Add sequence"
                        RibbonButton { commandId: "ribbon.type-a-new-sequence-or-paste-fasta"; text: "Type/paste"; iconName: "new-file"; tip: "Type a new sequence or paste FASTA"; enabled: window.current !== null && !window.current.busy; onInvoked: docs.itemAt(tabs.currentIndex).addText() }
                        RibbonButton { commandId: "ribbon.add-all-sequences-from-a-fasta-file"; text: "FASTA"; iconName: "add-file"; tip: "Add all sequences from a FASTA file"; enabled: window.current !== null && !window.current.busy; onInvoked: docs.itemAt(tabs.currentIndex).addFasta() }
                        RibbonButton { commandId: "ribbon.review-chromatograms-and-add-their-consensus"; text: "Chromatogram"; iconName: "waveform"; tip: "Review chromatograms and add their consensus"; enabled: window.current !== null && !window.current.busy; onInvoked: docs.itemAt(tabs.currentIndex).addTraces() }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                // Tree: the selected clade, the order of clades, support
                // values and figures.
                RibbonPage {
                    spacing: 6
                    SpeciesMethodsGroup { commandSuffix: "tree" }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Clade"
                        RibbonButton { commandId: "ribbon.name-the-selected-clade-with-a-bar-or-a"; text: "Annotate…"; tip: "Name the selected clade, with a bar or a coloured field"; keys: "Enter"; enabled: window.current !== null && window.current.has_selection === true; onInvoked: cladeEditor.openSelected() }
                        RibbonButton { commandId: "ribbon.root-the-tree-on-the-branch-above-the-selection"; text: "Reroot"; tip: "Root the tree on the branch above the selection"; enabled: window.current !== null && window.current.has_selection === true; onInvoked: window.current.reroot() }
                        RibbonButton { commandId: "ribbon.move-the-root-along-its-branch"; text: "Root position…"; tip: "Move the root along its branch"; onInvoked: rootDialog.ask() }
                        RibbonButton { commandId: "ribbon.reverse-the-order-of-the-branches-in-the-selected"; text: "Flip"; tip: "Reverse the order of the branches in the selected subtree (for a tip, its parent's)"; enabled: window.current !== null && window.current.has_selection === true; onInvoked: window.current.rotate() }
                        RibbonButton { commandId: "ribbon.rename-the-selected-tip"; text: "Rename…"; tip: "Rename the selected tip"; keys: "F2"; enabled: window.current !== null && window.current.tip_selected === true; onInvoked: tipDialog.ask() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Order"
                        RibbonButton { commandId: "ribbon.order-every-clade-s-branches-by-size-smallest-first"; text: "Small on top"; tip: "Order every clade's branches by size, smallest first"; onInvoked: window.current.ladderize(true) }
                        RibbonButton { commandId: "ribbon.order-every-clade-s-branches-by-size-largest-first"; text: "Large on top"; tip: "Order every clade's branches by size, largest first"; onInvoked: window.current.ladderize(false) }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Support"
                        RibbonButton { commandId: "ribbon.add-support-values-from-another-analysis-of-the-same"; text: "Add from tree…"; tip: "Add support values from another analysis of the same taxa, matched branch by branch"; onInvoked: supportFile.open() }
                        ModeButton { commandId: "ribbon.above-branches-going-up-below-branches-going-down"; text: "Natural"; tip: "Above branches going up, below branches going down"; active: window.current !== null && window.current.support_placement === "natural"; onInvoked: window.current.place_support("natural") }
                        ModeButton { commandId: "ribbon.all-support-values-above-their-branch"; text: "Above"; tip: "All support values above their branch"; active: window.current !== null && window.current.support_placement === "above"; onInvoked: window.current.place_support("above") }
                        ModeButton { commandId: "ribbon.all-support-values-below-their-branch"; text: "Below"; tip: "All support values below their branch"; active: window.current !== null && window.current.support_placement === "below"; onInvoked: window.current.place_support("below") }
                        RibbonButton { commandId: "ribbon.which-support-values-show-circles-at-nodes-sizes"; text: "Style…"; tip: "Which support values show, circles at nodes, sizes"; onInvoked: treeStyle.openStyle() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Linked alignment"
                        RibbonButton { commandId: "ribbon.associate-an-imported-tree-with-an-alignment-after-validating"; text: "Link alignment…"; tip: "Associate an imported tree with an alignment after validating its identifiers"; onInvoked: linkAlignmentFile.open() }
                        RibbonButton { commandId: "ribbon.extract-aligned-rows-without-realigning-ctrl-click-to-select"; text: "Open selected clade(s)"; enabled: window.current !== null && !window.current.busy; tip: "Extract aligned rows without realigning; Ctrl-click to select multiple clades"; onInvoked: window.current.extract_clade_alignment() }
                    }
                    RibbonGroup {
                        title: "Figure"
                        RibbonButton { commandId: "ribbon.save-the-complete-tree-as-an-editable-svg-with"; text: "Export SVG…"; tip: "Save the complete tree as an editable SVG with the current colours and font"; onInvoked: window.exportSvg() }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                // Build phylogeny: tree inference from the open alignment.
                RibbonPage {
                    spacing: 6
                    RibbonGroup {
                        title: "Distance tree"
                        Repeater {
                            model: [{label: "Matrix", value: "matrix"}, {label: "NJ", value: "nj"}, {label: "UPGMA", value: "upgma"}]
                            delegate: RibbonButton { commandId: "analysis.distance." + modelData.value;
                                required property var modelData
                                iconName: modelData.value === "matrix" ? "matrix" : modelData.value === "nj" ? "phylogram" : "tree"
                                text: modelData.label
                                enabled: window.current !== null && !window.current.busy
                                tip: modelData.value === "matrix" ? "Save observed distances as TSV and a reusable cache" : modelData.value === "upgma" ? "Build a rooted, equal-rate UPGMA tree" : "Build a neighbor-joining tree"
                                onInvoked: {
                                    const dialog = modelData.value === "matrix" ? matrixDialog : modelData.value === "nj" ? njDialog : upgmaDialog
                                    dialog.target = window.current
                                    dialog.open()
                                }
                            }
                        }
                        ColumnLayout {
                            visible: window.currentKind === "alignment" && window.current !== null && window.current.bootstrap_running
                            Label { text: parent.visible ? window.current.bootstrap_status : ""; font.pixelSize: 11 }
                            ProgressBar { from: 0; to: 1; value: parent.visible ? window.current.bootstrap_progress : 0; Layout.fillWidth: true }
                            AppButton { text: "Cancel bootstrap"; enabled: parent.visible && !window.current.bootstrap_cancelling; onClicked: window.current.cancel_bootstrap() }
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    SpeciesMethodsGroup { commandSuffix: "alignment" }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Maximum likelihood"
                        RibbonButton { commandId: "ribbon.infer-a-tree-with-fasttree-standard-settings-gtr-for";
                            iconName: "phylogram"
                            text: window.current && window.current.busy ? "Running…" : "FastTree"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Infer a tree with FastTree, standard settings (GTR for DNA, LG for protein), into a new file"
                            onInvoked: window.current.build_fasttree()
                        }
                        RibbonButton { commandId: "ribbon.select-a-model-and-infer-a-maximum-likelihood-tree";
                            iconName: "phylogram"
                            text: "IQ-TREE"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Select a model and infer a maximum-likelihood tree; allocated CPU threads, seed 1, no support resampling"
                            onInvoked: window.current.build_iqtree()
                        }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }
                RibbonPage {
                    SpeciesMethodsGroup { commandSuffix: "workspace" }
                    RibbonGroup {
                        title: "Display"
                        RibbonButton { commandId: "species.display"; text: "Tree / groups…"; iconName: "tree"; enabled: window.current !== null && !window.current.busy; onInvoked: window.beginDelimitation("view") }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }

                RibbonPage {
                    spacing: 6
                    RibbonGroup {
                        title: "Issues"
                        RibbonButton { commandId: "ribbon.previous-unresolved-issue"; iconName: "previous"; tip: "Previous unresolved issue"; keys: "Alt+← or ["; visible: commandAction.visible && (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.issue(false) }
                        RibbonButton { commandId: "ribbon.next-unresolved-issue"; iconName: "next"; tip: "Next unresolved issue"; keys: "Alt+→ or ]"; visible: commandAction.visible && (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.issue(true) }
                        RibbonButton { commandId: "ribbon.accept-this-base-and-go-to-the-next-issue"; text: "Accept"; iconName: "accept"; tip: "Accept this base and go to the next issue"; keys: "Alt+Enter"; visible: commandAction.visible && (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.accept() }
                        RibbonButton { commandId: "ribbon.choose-alignment-context"; text: "Alignment…"; tip: "Choose alignment context"; keys: "Ctrl+Alt+A"; visible: commandAction.visible && (window.traceOpen && window.current && window.current.review_mode && window.traceDocument && window.traceDocument.returnAlignment === null); onInvoked: window.traceDocument.alignmentContext() }
                        RibbonButton { commandId: "ribbon.combine-up-to-three-reads"; text: "Draft reads…"; tip: "Combine up to three reads"; keys: "Ctrl+Alt+D"; visible: commandAction.visible && (window.traceOpen && window.current && !window.current.review_mode); onInvoked: window.traceDocument.draftReads() }
                        RibbonButton { commandId: "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft"; text: "Signal cutoff…"; tip: "Choose the terminal signal percentage for a new draft"; onInvoked: window.traceDocument.signalSettings() }
                    }
                    RibbonGroup {
                        title: "Masking"
                        visible: window.traceOpen && window.current && window.current.review_mode
                        RibbonButton { commandId: "ribbon.choose-the-consensus-range-to-keep-in-exports"; text: "Mask ends…"; tip: "Choose the consensus range to keep in exports"; keys: "Ctrl+Alt+M"; onInvoked: window.traceDocument.maskEnds() }
                        RibbonButton { commandId: "ribbon.find-partial-terminal-primer-candidates-and-mask-them"; text: "Set primers…"; tip: "Find partial terminal primer candidates and mask them"; keys: "Ctrl+Alt+P"; onInvoked: window.traceDocument.setPrimers() }
                    }
                    RibbonGroup {
                        title: "Export consensus"
                        visible: window.traceOpen && window.current && window.current.review_mode
                        ExportConsensusButton { commandId: "ribbon.add-the-edited-consensus-to-its-alignment"; text: "to alignment"; tip: "Add the edited consensus to its alignment"; keys: "Ctrl+Shift+A"; visible: commandAction.visible && (window.traceDocument && window.traceDocument.returnAlignment !== null); enabled: window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.addToAlignment() }
                        ExportConsensusButton { commandId: "ribbon.export-the-edited-consensus-as-fasta"; text: "to fasta"; tip: "Export the edited consensus as FASTA"; keys: "Ctrl+Shift+E"; enabled: window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.exportConsensus() }
                        ExportConsensusButton { commandId: "ribbon.copy-the-edited-consensus-as-fasta"; text: "to clipboard"; tip: "Copy the edited consensus as FASTA"; keys: "Ctrl+Shift+C"; enabled: window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.copyConsensus() }
                    }
                    Item { Layout.fillWidth: true }
                    ShortcutsGroup {}
                }
        }
    }

    DistanceAnalysisDialog { id: matrixDialog; kind: "matrix" }
    DistanceAnalysisDialog { id: njDialog; kind: "nj" }
    DistanceAnalysisDialog { id: upgmaDialog; kind: "upgma" }

    ShortcutsDialog { id: shortcuts; commands: window.applicationCommands }

    AppContentDialog {
        id: orfDialog
        initialFocusItem: codons
        property bool inSelection: false
        function search(selectionOnly) { inSelection = selectionOnly; open() }
        title: inSelection ? "Find open reading frames in the selection" : "Find open reading frames"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: codons.forceActiveFocus()
        onAccepted: {
            window.orfMinCodons = codons.value
            window.current.find_orfs(codons.value, orfDialog.inSelection)
        }
        body: ColumnLayout {
            spacing: 8
            RowLayout {
                Label { id: codonsLabel; text: "At least" }
                SpinBox {
                    id: codons
                    Accessible.labelledBy: codonsLabel
                    Accessible.name: codonsLabel.text + " (codons, start to stop)"
                    from: 10
                    to: 10000
                    stepSize: 10
                    editable: true
                    value: window.orfMinCodons
                    Keys.onReturnPressed: orfDialog.accept()
                    Keys.onEnterPressed: orfDialog.accept()
                }
                Label { text: "codons, start to stop" }
            }
            Label {
                text: "Searches both strands. Each ORF is added as a misc_feature with the note \"ORF\";\none Undo removes them all."
                opacity: 0.7
            }
        }
    }

    AppContentDialog {
        id: genesDialog
        initialFocusItem: codeBox
        property bool inSelection: false
        function predict(selectionOnly) {
            inSelection = selectionOnly
            const code = window.current.genetic_code()
            let i = 0
            for (let k = 0; k < geneticCodes.count; k++)
                if (geneticCodes.get(k).code === code) i = k
            codeBox.currentIndex = i
            open()
        }
        title: inSelection ? "Predict genes in the selection" : "Predict genes"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: codeBox.forceActiveFocus()
        onAccepted: window.current.predict_genes(geneticCodes.get(codeBox.currentIndex).code, inSelection)
        body: ColumnLayout {
            spacing: 8
            RowLayout {
                Label { id: codeBoxLabel; text: "Genetic code" }
                AppComboBox {
                    id: codeBox; objectName: "codeBox"; Accessible.labelledBy: codeBoxLabel; Accessible.name: codeBoxLabel.text
                    Layout.fillWidth: true
                    model: geneticCodes
                    textRole: "label"
                    Keys.onReturnPressed: genesDialog.accept()
                    Keys.onEnterPressed: genesDialog.accept()
                }
            }
            Label {
                text: "Prodigal trains on sequences of 20 kb or more. Shorter ones use its preset models,\nwhich exist only for codes 4 and 11. Genes are added as CDS; one Undo removes them all."
                opacity: 0.7
            }
        }
        // NCBI genetic codes that Prodigal supports.
        ListModel {
            id: geneticCodes
            ListElement { code: 1; label: "1  Standard" }
            ListElement { code: 2; label: "2  Vertebrate mitochondrial" }
            ListElement { code: 3; label: "3  Yeast mitochondrial" }
            ListElement { code: 4; label: "4  Mold, protozoan and coelenterate mitochondrial; Mycoplasma" }
            ListElement { code: 5; label: "5  Invertebrate mitochondrial" }
            ListElement { code: 6; label: "6  Ciliate, dasycladacean and Hexamita nuclear" }
            ListElement { code: 9; label: "9  Echinoderm and flatworm mitochondrial" }
            ListElement { code: 10; label: "10  Euplotid nuclear" }
            ListElement { code: 11; label: "11  Bacterial, archaeal and plant plastid" }
            ListElement { code: 12; label: "12  Alternative yeast nuclear" }
            ListElement { code: 13; label: "13  Ascidian mitochondrial" }
            ListElement { code: 14; label: "14  Alternative flatworm mitochondrial" }
            ListElement { code: 15; label: "15  Blepharisma nuclear" }
            ListElement { code: 16; label: "16  Chlorophycean mitochondrial" }
            ListElement { code: 21; label: "21  Trematode mitochondrial" }
            ListElement { code: 22; label: "22  Scenedesmus obliquus mitochondrial" }
            ListElement { code: 23; label: "23  Thraustochytrium mitochondrial" }
            ListElement { code: 24; label: "24  Rhabdopleuridae mitochondrial" }
            ListElement { code: 25; label: "25  Candidate division SR1 and Gracilibacteria" }
            ListElement { code: 26; label: "26  Pachysolen tannophilus nuclear" }
            ListElement { code: 29; label: "29  Mesodinium nuclear" }
            ListElement { code: 30; label: "30  Peritrich nuclear" }
            ListElement { code: 32; label: "32  Balanophoraceae plastid" }
            ListElement { code: 33; label: "33  Cephalodiscidae mitochondrial" }
        }
    }

    AppContentDialog {
        id: about
        title: "About Basecraft"
        modal: true
        standardButtons: Dialog.Close
        body: ColumnLayout {
            spacing: 6
            Label { text: "Basecraft " + shell.version(); font.bold: true }
            Label { text: "A workbench for sequences and their annotations." }
            Label { text: "Edits support undo and replay. Recovery records are kept in application state."; opacity: 0.7 }
            Label { text: "GPL-3.0-or-later; plugin protocol and Python SDK Apache-2.0."; opacity: 0.7 }
        }
    }

    FileDialog {
        id: openDialog
        title: "Open a sequence, alignment or tree"
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Sequences, alignments, traces and trees (*.gb *.gbk *.genbank *.gbff *.fasta *.fa *.fna *.fas *.ffn *.faa *.afa *.ab1 *.abi *.nwk *.newick *.tre *.tree *.trees *.treefile *.contree *.nex *.nexus *.nxs)", "All files (*)"]
        onAccepted: window.openPath(window.localPath(selectedFile))
    }

    // Save the active tab under a new name; the tab then shows the new file.
    function saveAs() {
        if (!window.current) return
        if (window.traceOpen) { window.traceDocument.saveReview(); return }
        saveDialog.index = tabs.currentIndex
        saveDialog.selectedFile = shell.local_file_url(window.currentPath)
        saveDialog.open()
    }

    Shortcut { sequence: "Ctrl+Q"; onActivated: window.close() }
    // Close the active tab, asking first if it has unsaved changes.
    Shortcut { sequence: "Ctrl+W"; onActivated: if (openFiles.count > 0) window.closeTab(tabs.currentIndex) }
    FileDialog {
        id: saveDialog
        property int index: -1
        title: "Save as"
        fileMode: FileDialog.SaveFile
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        onAccepted: {
            const d = docs.itemAt(index)
            const path = window.localPath(selectedFile)
            if (d && d.controller.save_as(path)) {
                openFiles.setProperty(index, "path", path)
                Qt.callLater(window.refreshCurrent)
            }
        }
    }

    FolderDialog {
        id: projectDialog
        title: "Open or create a project folder"
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        onAccepted: window.selectProject(window.localPath(selectedFolder))
    }

    SplitView {
        anchors.fill: parent
        anchors.topMargin: 4
        orientation: Qt.Horizontal

        // The project's files, from Qt's QFileSystemModel; collapsible.
        Rectangle {
            id: sidebar
            visible: window.sidebarOpen
            SplitView.preferredWidth: 200
            SplitView.minimumWidth: 120
            color: Qt.alpha(shell.foreground, 0.03)

            FileTreeModel {
                id: files
                rootDir: shell.project_dir
            }

            Label {
                id: projectName
                anchors { top: parent.top; left: parent.left; right: collapse.left; margins: 8 }
                text: shell.project_dir.length ? window.fileName(shell.project_dir) : "No project"
                font.bold: true
                elide: Text.ElideMiddle
            }
            AppIconButton {
                id: collapse
                anchors { verticalCenter: projectName.verticalCenter; right: parent.right; rightMargin: 2 }
                iconName: "left"; description: "Hide files"
                implicitWidth: 22
                implicitHeight: 22
                focusPolicy: Qt.StrongFocus
                ToolTip.text: "Hide files (Ctrl+B)"
                ToolTip.visible: hovered
                onClicked: window.sidebarOpen = false
            }

            TreeView {
                id: tree
                anchors { top: projectName.bottom; topMargin: 6; left: parent.left; right: parent.right; bottom: parent.bottom }
                clip: true
                model: files
                rootIndex: files.rootIndex
                selectionModel: ItemSelectionModel {}
                delegate: FileTreeDelegate {
                    currentPath: window.currentPath
                    foreground: shell.foreground
                    onOpenRequested: (path) => window.openPath(path)
                }
                ScrollBar.vertical: ScrollBar {}
            }
        }

        // Collapsed sidebar: a thin strip to bring it back.
        Rectangle {
            visible: !window.sidebarOpen
            SplitView.preferredWidth: 18
            SplitView.minimumWidth: 18
            SplitView.maximumWidth: 18
            color: Qt.alpha(shell.foreground, 0.03)
            AppIconButton {
                anchors { top: parent.top; horizontalCenter: parent.horizontalCenter; topMargin: 4 }
                iconName: "right"; description: "Show files"
                implicitWidth: 18
                implicitHeight: 22
                focusPolicy: Qt.StrongFocus
                ToolTip.text: "Show files (Ctrl+B)"
                ToolTip.visible: hovered
                onClicked: window.sidebarOpen = true
            }
        }

        // The open files: tabs, the active document, its status line.
        ColumnLayout {
            SplitView.fillWidth: true
            spacing: 0

            TabBar {
                id: tabs
                Layout.fillWidth: true
                visible: openFiles.count > 0
                onCurrentIndexChanged: Qt.callLater(window.refreshCurrent)
                Repeater {
                    model: openFiles
                    AppTabButton {
                        required property int index
                        required property string path
                        required property bool dirty
                        onClicked: { window.focusOpenedDocument = true; Qt.callLater(window.refreshCurrent) }
                        Accessible.name: window.fileName(path) + (dirty ? ", unsaved changes" : "")
                        width: implicitWidth
                        focusPolicy: Qt.StrongFocus
                        contentItem: Row {
                            spacing: 6
                            Label {
                                // Long names shorten in the middle, keeping
                                // the extension; the path in the project is a
                                // hover away.
                                text: window.fileName(path) + (dirty ? " •" : "")
                                width: Math.min(implicitWidth, 180)
                                elide: Text.ElideMiddle
                                anchors.verticalCenter: parent.verticalCenter
                                ToolTip.text: shell.relative_path(path)
                                ToolTip.visible: tabHover.hovered
                                HoverHandler { id: tabHover }
                            }
                            AppIconButton {
                                iconName: "close"; description: "Close tab"
                                icon.width: 8
                                icon.height: 8
                                implicitWidth: 18
                                implicitHeight: 18
                                padding: 0
                                focusPolicy: Qt.StrongFocus
                                anchors.verticalCenter: parent.verticalCenter
                                onClicked: window.closeTab(index)
                            }
                        }
                    }
                }
            }

            StackLayout {
                id: stack
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: tabs.currentIndex

                Repeater {
                    id: docs
                    model: openFiles
                    onItemAdded: Qt.callLater(window.refreshCurrent)
                    onItemRemoved: Qt.callLater(window.refreshCurrent)
                    // Each file opens in the editor for what it holds.
                    delegate: DelegateChooser {
                        role: "kind"
                        DelegateChoice {
                            roleValue: "trace"
                            SangerView {
                                notificationSink: provenance
                                fileUrls: shell
                                onCopyText: (text) => clipboard.put(text)
                                onRedraft: (paths, reference, target, cutoff) => window.startTraceReview(paths, reference, target, cutoff, true)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                            }
                        }
                        DelegateChoice {
                            roleValue: "alignment"
                            AlignmentView {
                                notificationSink: provenance
                                fileUrls: shell
                                insertMode: window.alignmentInsert
                                onActivate: tabs.currentIndex = index
                                onReviewTraces: (paths, reference) => {
                                    window.startTraceReview(paths, reference, docs.itemAt(tabs.currentIndex))
                                }
                                onToggleInsert: window.alignmentInsert = !window.alignmentInsert
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { alnMenu.info = info; alnMenu.popup() }
                                onCopyText: (text) => clipboard.put(text)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameRow: renameDialog.ask()
                            }
                        }
                        DelegateChoice {
                            roleValue: "tree"
                            // Not "TreeView": that's Qt's, used by the
                            // files sidebar.
                            PhyloView {
                                notificationSink: provenance
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { treeMenu.info = info; treeMenu.popup() }
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameTip: tipDialog.ask()
                                onAnnotate: cladeEditor.openSelected()
                            }
                        }
                        DelegateChoice {
                            roleValue: "delimitation"
                            DelimitationView {
                                notificationSink: provenance
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                            }
                        }
                        DelegateChoice {
                            DocumentView {
                                notificationSink: provenance
                                showGc: window.showGc
                                defaultLayout: window.defaultLayout
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { menu.info = info; menu.popup() }
                                onEditFeature: (id) => editor.openFeature(id)
                                onEditSelected: editor.openSelected()
                                onNewAnnotation: editor.openNew()
                                onCopyText: (text) => clipboard.put(text)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                            }
                        }
                    }
                }
            }

            Label {
                visible: openFiles.count === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                text: "Open a sequence or an alignment from the files on the left"
                opacity: 0.6
            }

            // The status line belongs to the open file, under it only.
            Rectangle {
                visible: openFiles.count > 0
                Layout.fillWidth: true
                implicitHeight: 20
                color: Qt.alpha(shell.foreground, 0.06)
                RowLayout {
                    anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
                    spacing: 12
                    Label {
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        text: window.current ? window.current.status : ""
                    }
                    Label {
                        readonly property string hoverText: window.alignmentDocument ? window.alignmentDocument.hoverStatus
                            : window.traceDocument ? window.traceDocument.hoverStatus : ""
                        visible: hoverText.length > 0
                        text: hoverText
                        Layout.maximumWidth: parent.width * 0.6
                        elide: Text.ElideLeft
                    }
                }
            }
        }
    }

    // Only what applies where you clicked: an annotation, or the selection.
    // Global actions (zoom, whole sequence) live in the toolbar.
    Menu {
        id: menu
        property var info: ({ feature: -1, label: "", featureLength: 0, selection: false, length: 0 })
        readonly property bool onAnnotation: info.feature >= 0
        component Item: MenuItem { height: visible ? implicitHeight : 0 }

        Item { text: "Edit “" + menu.info.label + "”…"; visible: menu.onAnnotation; onTriggered: editor.openFeature(menu.info.feature) }
        Item { text: "Delete “" + menu.info.label + "”"; visible: menu.onAnnotation; onTriggered: window.current.delete_feature(menu.info.feature) }
        Item {
            text: "Copy annotation sequence (" + menu.info.featureLength + " bp)"
            visible: menu.onAnnotation
            onTriggered: clipboard.put(window.current.feature_sequence(menu.info.feature))
        }
        Item { text: "Extract as new sequence"; visible: menu.onAnnotation; onTriggered: window.current.extract_feature(menu.info.feature) }
        Item { text: "Zoom to annotation"; visible: menu.onAnnotation; onTriggered: window.current.zoom_to_feature(menu.info.feature) }

        Item { text: "Add annotation…"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: editor.openNew() }
        Item {
            text: "Copy sequence (" + menu.info.length + " bp)"
            visible: !menu.onAnnotation
            enabled: menu.info.selection
            onTriggered: clipboard.put(window.current.selection_text())
        }
        Item { text: "Extract as new sequence"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: window.current.extract_selection() }
        Item { text: "Zoom to selection"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: window.current.zoom_to_selection() }
        Item { text: "Find ORFs in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: orfDialog.search(true) }
        Item { text: "Predict genes in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: genesDialog.predict(true) }
    }

    // Alignment context menu: what applies to the cells, rows or columns
    // under the pointer.
    Menu {
        id: alnMenu
        property var info: ({ region: "grid", rows: 1, columns: 1, rowName: "" })
        readonly property bool cells: info.region === "grid"
        readonly property bool rows: info.region === "names" || info.region === "corner"
        readonly property bool columns: info.region === "header"
        component AlnItem: MenuItem { height: visible ? implicitHeight : 0 }

        AlnItem { text: "Insert gap"; visible: alnMenu.cells; onTriggered: window.current.insert_gap() }
        AlnItem { text: "Remove gap to the left"; visible: alnMenu.cells; onTriggered: window.current.remove_gap() }
        AlnItem { text: "Delete " + alnMenu.info.rows + " × " + alnMenu.info.columns + " bases"; visible: alnMenu.cells; onTriggered: window.current.delete_selection() }
        AlnItem { text: "Rename “" + alnMenu.info.rowName + "”…"; visible: alnMenu.info.rows === 1 && !alnMenu.columns; onTriggered: renameDialog.ask() }
        AlnItem { text: "Move to top"; visible: alnMenu.rows; onTriggered: window.current.rows_to_top() }
        AlnItem { text: alnMenu.info.rows === 1 ? "Delete row" : "Delete " + alnMenu.info.rows + " rows"; visible: alnMenu.rows; onTriggered: window.current.remove_rows() }
        AlnItem { text: "Insert gap column"; visible: alnMenu.columns; onTriggered: window.current.insert_column() }
        AlnItem { text: alnMenu.info.columns === 1 ? "Delete column" : "Delete " + alnMenu.info.columns + " columns"; visible: alnMenu.columns; onTriggered: window.current.delete_columns() }
        AlnItem { text: "Strip gap-only columns"; visible: alnMenu.columns || alnMenu.rows; onTriggered: window.current.remove_gap_columns() }
        AlnItem { text: "Copy as FASTA"; onTriggered: clipboard.put(window.current.selection_text()) }
    }

    AppContentDialog {
        id: renameDialog
        initialFocusItem: rowName
        title: "Rename row"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        function ask() {
            if (!window.alignmentOpen) return
            rowName.text = window.current.row_name()
            renameError.text = ""
            open()
        }
        onOpened: { rowName.forceActiveFocus(); rowName.selectAll() }
        onAccepted: {
            const err = window.current.rename_row(rowName.text)
            if (err.length > 0) { renameError.text = err; open() }
        }
        body: ColumnLayout {
            spacing: 6
            Label { id: rowNameLabel; text: "Row name" }
            TextField {
                Accessible.labelledBy: rowNameLabel
                Accessible.name: rowNameLabel.text
                id: rowName
                Layout.preferredWidth: 320
                Keys.onReturnPressed: renameDialog.accept()
                Keys.onEnterPressed: renameDialog.accept()
            }
            AppErrorLabel { id: renameError; visible: text.length > 0; color: shell.accent; wrapMode: Text.Wrap; Layout.fillWidth: true }
        }
    }

    // Tree context menu: what applies to the clade or tip under the pointer.
    Menu {
        id: treeMenu
        property var info: ({ node: false, tip: false, root: false, annotated: false, annotation: 0 })
        component TreeItem: MenuItem { height: visible ? implicitHeight : 0 }

        TreeItem { text: treeMenu.info.annotated ? "Edit annotation…" : "Annotate clade…"; visible: treeMenu.info.node; onTriggered: cladeEditor.openSelected() }
        TreeItem { text: "Remove annotation"; visible: treeMenu.info.annotated; onTriggered: window.current.remove_annotation(treeMenu.info.annotation) }
        TreeItem { text: "Reroot here"; visible: treeMenu.info.node && !treeMenu.info.root; onTriggered: window.current.reroot() }
        TreeItem { text: "Root position…"; visible: treeMenu.info.rootBranch === true; onTriggered: rootDialog.ask() }
        TreeItem { text: "Flip subtree"; visible: treeMenu.info.node; onTriggered: window.current.rotate() }
        TreeItem { text: "Rename tip…"; visible: treeMenu.info.tip; onTriggered: tipDialog.ask() }
        TreeItem { text: "Add support from tree…"; visible: !treeMenu.info.node; onTriggered: supportFile.open() }
        TreeItem { text: "Tree style…"; visible: !treeMenu.info.node; onTriggered: treeStyle.openStyle() }
    }

    // Where the root sits along its branch: a share of the branch on each
    // side. Halfway after rerooting.
    AppContentDialog {
        id: rootDialog
        initialFocusItem: rootShare
        property var info: ({ side: "", other: "", length: 0, share: 0.5 })
        title: "Root position"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        function ask() {
            if (!window.treeOpen) return
            const i = JSON.parse(window.current.root_json())
            if (!i) {
                window.current.message_kind = "error"
                window.current.message = "The root needs two branches with lengths"
                return
            }
            info = i
            rootShare.value = i.share * 100
            open()
        }
        function length(share) { return (info.length * share).toPrecision(3) }
        onAccepted: window.current.move_root(rootShare.value / 100)
        body: ColumnLayout {
            spacing: 8
            Label { id: rootShareLabel; text: "Share of the root branch on each side"; opacity: 0.7 }
            GridLayout {
                columns: width < 600 ? 1 : 3
                Layout.fillWidth: true
                Label { text: rootDialog.info.side; elide: Text.ElideMiddle; Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true; horizontalAlignment: Text.AlignRight }
                Slider { id: rootShare; Accessible.labelledBy: rootShareLabel; Accessible.name: rootShareLabel.text + " (%)"; from: 0; to: 100; stepSize: 0.5; Layout.preferredWidth: 220; Layout.minimumWidth: 0; Layout.fillWidth: true }
                Label { text: rootDialog.info.other; elide: Text.ElideMiddle; Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true }
            }
            GridLayout {
                columns: width < 600 ? 1 : 3
                Layout.fillWidth: true
                Label {
                    text: rootShare.value.toFixed(1) + " % · " + rootDialog.length(rootShare.value / 100)
                    Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true
                    horizontalAlignment: Text.AlignRight
                }
                AppButton { text: "Halfway"; flat: true; Layout.preferredWidth: 220; Layout.minimumWidth: 0; Layout.fillWidth: true; onClicked: rootShare.value = 50 }
                Label { text: (100 - rootShare.value).toFixed(1) + " % · " + rootDialog.length(1 - rootShare.value / 100); Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true }
            }
        }
    }

    AppContentDialog {
        id: tipDialog
        initialFocusItem: tipName
        title: "Rename tip"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        function ask() {
            if (!window.treeOpen || !window.current.tip_selected) return
            tipName.text = window.current.tip_name()
            tipError.text = ""
            open()
        }
        onOpened: { tipName.forceActiveFocus(); tipName.selectAll() }
        onAccepted: {
            const err = window.current.rename_tip(tipName.text)
            if (err.length > 0) { tipError.text = err; open() }
        }
        body: ColumnLayout {
            spacing: 6
            Label { id: tipNameLabel; text: "Tip name" }
            TextField {
                Accessible.labelledBy: tipNameLabel
                Accessible.name: tipNameLabel.text
                id: tipName
                Layout.preferredWidth: 320
                Keys.onReturnPressed: tipDialog.accept()
                Keys.onEnterPressed: tipDialog.accept()
            }
            AppErrorLabel { id: tipError; visible: text.length > 0; color: shell.accent; wrapMode: Text.Wrap; Layout.fillWidth: true }
        }
    }

    // Support from another tree: pick the file, then name the values.
    FileDialog {
        id: supportFile
        title: "Add support values from another tree"
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Trees (*.nwk *.newick *.tre *.tree *.trees *.treefile *.contree *.nex *.nexus *.nxs)", "All files (*)"]
        onAccepted: { supportName.path = window.localPath(selectedFile); supportName.open() }
    }
    AppContentDialog {
        id: supportName
        initialFocusItem: supportNameField
        property string path
        title: "Name the support values"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: { supportNameField.text = ""; supportNameField.forceActiveFocus() }
        onAccepted: window.current.add_support_from(path, supportNameField.text)
        body: ColumnLayout {
            spacing: 6
            Label { text: window.fileName(supportName.path) }
            Label { id: supportNameFieldLabel; text: "Support values name" }
            TextField {
                Accessible.labelledBy: supportNameFieldLabel
                Accessible.name: supportNameFieldLabel.text
                id: supportNameField
                Layout.preferredWidth: 320
                placeholderText: "e.g. Bootstrap, PP, UFBoot; empty keeps the file's names"
                Keys.onReturnPressed: supportName.accept()
                Keys.onEnterPressed: supportName.accept()
            }
        }
    }

    CladeEditor {
        id: cladeEditor
        controller: window.treeOpen ? window.current : null
    }
    TreeStyleDialog {
        id: treeStyle
        controller: window.treeOpen ? window.current : null
    }

    FileDialog {
        id: linkAlignmentFile
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        title: "Link an alignment to this tree"
        nameFilters: ["FASTA alignments (*.fasta *.fa *.fas *.afa)"]
        onAccepted: if (window.treeOpen) window.current.link_alignment(window.localPath(selectedFile))
    }

    function exportSvg() {
        if (!window.current) return
        svgDialog.controller = window.current
        svgScope.currentIndex = window.treeOpen ? 2 : 0
        svgPublication.checked = false
        svgOptions.open()
    }
    AppContentDialog {
        id: svgOptions
        initialFocusItem: svgScope
        title: "Export editable SVG"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        body: ColumnLayout {
            Label { id: svgScopeLabel; text: "Region" }
            AppComboBox { id: svgScope; objectName: "svgScope"; Accessible.labelledBy: svgScopeLabel; Accessible.name: svgScopeLabel.text; model: ["Visible view", "Selection", "Complete figure"] }
            CheckBox { id: svgPublication; text: "Publication colours (white background)" }
        }
        onAccepted: {
            svgDialog.selectedFile = shell.local_file_url(window.currentPath.replace(/\.[^./]*$/, "") + ".svg")
            svgDialog.open()
        }
    }
    FileDialog {
        id: svgDialog
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        property var controller: null
        title: "Export SVG"
        fileMode: FileDialog.SaveFile
        nameFilters: ["SVG figures (*.svg)"]
        onAccepted: if (controller) controller.export_figure(window.localPath(selectedFile), svgScope.currentIndex, svgPublication.checked)
    }

    AppContentDialog {
        id: draftQuitDialog
        title: "Unsaved methods drafts"
        standardButtons: Dialog.Discard | Dialog.Cancel
        body: ColumnLayout {
            Label { text: "Save or copy your authored methods before quitting."; Layout.fillWidth: true; wrapMode: Text.Wrap }
            Repeater {
                model: draftQuitDialog.visible ? documentRecord.unsavedDrafts() : []
                AppButton {
                    required property string modelData
                    text: "Review " + window.fileName(modelData)
                    onClicked: { draftQuitDialog.close(); window.openPath(modelData); Qt.callLater(() => documentRecord.showPage(2)) }
                }
            }
        }
        onDiscarded: { window.discardAuthoredOnQuit = true; window.close() }
    }

    // Quitting with unsaved changes: save them all, throw them away, or
    // stay.
    AppFormDialog {
        id: quitDialog
        onRejected: window.discardAuthoredOnQuit = false
        property var unsaved: []
        title: "Save changes before closing?"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
        function ask(indexes) {
            unsaved = indexes
            quitError.text = ""
            open()
        }
        ColumnLayout {
            Kirigami.FormData.isSection: true
            Layout.fillWidth: true
            spacing: 6
            Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: quitDialog.unsaved.length === 1 ? "This file has unsaved changes:" : "These files have unsaved changes:" }
            Repeater {
                model: quitDialog.unsaved
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    required property int modelData
                    text: "  " + window.fileName(openFiles.get(modelData).path)
                    font.bold: true
                }
            }
            AppErrorLabel { id: quitError; visible: text.length > 0; color: shell.accent; wrapMode: Text.Wrap }
        }
        onAccepted: {
            for (const i of unsaved) docs.itemAt(i).controller.save()
            // A save that failed leaves its tab unsaved: stay open and say so.
            const left = window.unsavedTabs()
            if (left.length > 0) {
                unsaved = left
                quitError.text = "Couldn't save " + left.map(i => window.fileName(openFiles.get(i).path)).join(", ")
                    + ": " + docs.itemAt(left[0]).controller.message
                open()
                return
            }
            window.closeConfirmed = true
            window.close()
        }
        onDiscarded: {
            close()
            window.closeConfirmed = true
            window.close()
        }
    }

    AppFormDialog {
        id: confirmClose
        property int index: -1
        property string saveError: ""
        title: "Save changes?"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
        AppErrorLabel {
            Accessible.role: confirmClose.saveError.length ? Accessible.AlertMessage : Accessible.StaticText
            Kirigami.FormData.isSection: true
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: confirmClose.saveError.length ? confirmClose.saveError : "This file has unsaved changes."
        }
        onAccepted: {
            const controller = docs.itemAt(index).controller
            controller.save()
            if (controller.dirty) {
                saveError = controller.message || "Save the review to a file first"
                open()
                return
            }
            saveError = ""
            openFiles.remove(index)
            Qt.callLater(window.refreshCurrent)
        }
        onDiscarded: {
            close()
            openFiles.remove(index)
            Qt.callLater(window.refreshCurrent)
        }
    }

    FeatureEditor {
        id: editor
        // Annotations belong to sequences; alignments have none yet.
        controller: window.currentKind === "sequence" ? window.current : null
    }

    Timer {
        interval: 1500
        running: true
        repeat: true
        onTriggered: {
            shell.check_theme()
            for (let i = 0; i < docs.count; i++) {
                const d = docs.itemAt(i)
                if (d) {
                    d.controller.check_theme()
                    if (d.kind !== "trace") d.controller.flush()
                }
            }
        }
    }

    function arg(name) {
        const args = Qt.application.arguments
        const i = args.indexOf(name)
        return i >= 0 ? args[i + 1] : undefined
    }

    Component.onCompleted: {
        if (arg("--screenshot")) {
            if (arg("--window-width")) width = Number(arg("--window-width"))
            if (arg("--font-size")) font.pixelSize = Number(arg("--font-size"))
        }
        loadPreferences()
        updateRecentProjects(preferences.project_history())
        computeReady = true
        localCpuThreads = Math.max(1, Math.min(availableCpuThreads, localCpuThreads))
        shell.configure_cpu_threads(localCpuThreads)
        if (arg("--screenshot")) console.debug("PREFERENCES_AUDIT", JSON.stringify({error: preferences.error, modal: preferencesError.visible, ready: preferencesReady, font: font.family, fontPoints: font.pointSize, defaultFontPoints: defaultFontSizePoints}))
        const args = Qt.application.arguments
        for (let i = 1; i < args.length; i++) {
            if (args[i].startsWith("--")) { i++; continue }
            shell.use_project_of(args[i])
            openPath(args[i])
        }
    }

    property var delimPending: []
    function runDelimSteps(c) {
        while (window.delimPending.length && !c.busy) {
            const step=window.delimPending.shift()
            if (step.action === "start") docs.itemAt(tabs.currentIndex).startAnalysis(step.method, step.input, step.rooted || false, JSON.stringify(step.settings || {}), step.display || "existing")
            else if (step.action === "attach") c.attach_tree(step.path,step.rooted)
            else if (step.action === "select") c.highlight_taxa(JSON.stringify(step.ids))
            else if (step.action === "activate_last") { const work = JSON.parse(c.workspace_json); c.activate(work.hypotheses[work.hypotheses.length-1].id) }
            else if (step.action === "cut") c.preview_cut(step.threshold)
            else if (step.action === "cancel_cut") c.cancel_cut()
            else if (step.action === "save_cut") c.save_cut(step.name)
            else if (step.action === "undo") c.undo()
            else if (step.action === "redo") c.redo()
            else if (step.action === "save") c.save()
            else if (step.action === "run") c.run_analysis(step.method,step.input,step.rooted || false,JSON.stringify(step.settings || {}))
            else if (step.action === "statistics_csv") c.export_statistics(step.path)
            else if (step.action === "inspect") c.export_analysis(step.path)
            else if (step.action === "svg") c.export_svg(step.path)
            else if (step.action === "visible_all") c.action("visible",JSON.stringify({ids:JSON.parse(c.workspace_json).hypotheses.map(h=>h.id)}))
            else c.action(step.action,JSON.stringify(step))
            console.log("delim",step.action,"|",c.message,"|",c.workspace_json,"| preview",c.preview_json)
        }
    }

    // Scripted runs, on the active tab: --goto START,END  --mode wrapped|linear|circular
    // --type TEXT  --select X0,X1[,Y]  --menu X,Y  --menu-add X  --edit new|ID
    // --label TEXT [--no-click 1]  --add LOCATION  --extract 1  --hover X,Y
    // --no-sidebar 1  --fit 1  --aln STEPS  --keys KEYS  --shot-delay MS
    // --screenshot FILE
    Timer {
        interval: 900
        running: arg("--screenshot") !== undefined
        onTriggered: {
            const c = window.current
            const v = window.currentView
            if (!c) { console.log("no document open"); shot.start(); return }
            if (arg("--trace-review")) {
                window.startTraceReview(JSON.parse(arg("--trace-review")), c.review_snapshot(), docs.itemAt(tabs.currentIndex))
                shot.start()
                return
            }
            if (arg("--zoom-percent")) {
                const controller = window.delimitationOpen ? v.controller : c
                controller.set_zoom_percent(Number(arg("--zoom-percent")))
                console.log("ZOOM_PERCENT", controller.zoom_text)
            }
            if (arg("--cpu-threads")) window.localCpuThreads = Number(arg("--cpu-threads"))
            if (arg("--font-points")) preferences.update("fontSizePoints", arg("--font-points"))
            if (arg("--sanger-draft")) c.open_draft(arg("--sanger-draft"))
            if (arg("--sanger-single")) c.single_review(Number(arg("--signal-cutoff") || 20))
            if (arg("--sanger-heights")) JSON.parse(arg("--sanger-heights")).forEach((height, i) => c.trace_height(i, height))
            if (arg("--sanger-mask")) { const bounds = JSON.parse(arg("--sanger-mask")); c.mask_ends(bounds[0], bounds[1]) }
            if (arg("--sanger-action")) {
                ribbonTabName = "Trace"
                const trace = docs.itemAt(tabs.currentIndex)
                if (arg("--sanger-action") === "name") { trace.accept(); trace.copyConsensus() }
                if (arg("--sanger-action") === "mask") trace.maskEnds()
                if (arg("--sanger-action") === "signal") trace.signalSettings()
                if (arg("--sanger-action") === "primers") trace.setPrimers()
                if (arg("--sanger-action") === "save") trace.saveReview()
                if (arg("--sanger-action") === "issue") trace.issue(true)
                if (arg("--sanger-action") === "accept") trace.accept()
                if (arg("--sanger-action") === "export") { trace.accept(); trace.exportConsensus(); trace.confirmExportName() }
                if (arg("--sanger-action") === "copy") { trace.accept(); trace.copyConsensus(); trace.confirmExportName(); const expected = clipboard.text; clipboard.text = ""; clipboard.paste(); console.log("consensus clipboard", clipboard.text, "verified", clipboard.text === expected) }
            }
            if (arg("--sanger-reference")) c.place_alignment(arg("--sanger-reference"))
            if (arg("--review-thresholds")) c.apply_review_settings(arg("--review-thresholds"))
            if (arg("--add-sequences")) {
                const options = JSON.parse(arg("--add-sequences"))
                c.add_sequences(options.text, options.name || "", options.mafft !== false, options.orientation !== false, options.keep !== false)
            }
            if (arg("--add-text-dialog")) docs.itemAt(tabs.currentIndex).addText()
            if (arg("--add-fasta-dialog")) docs.itemAt(tabs.currentIndex).prepareFasta(arg("--add-fasta-dialog"))
            if (arg("--add-reviewed-dialog")) { const r = JSON.parse(arg("--add-reviewed-dialog")); docs.itemAt(tabs.currentIndex).addReviewed(r.sequence, r.name, r.mafft !== false) }
            if (arg("--fit")) c.fit()
            if (arg("--sanger-position")) c.press(Number(arg("--sanger-position")), 40)
            if (arg("--sanger-context")) {
                const point = arg("--sanger-context").split(",").map(Number)
                docs.itemAt(tabs.currentIndex).maskMenuAt(point[0], point[1])
            }
            if (arg("--delim")) {
                window.delimPending=JSON.parse(arg("--delim"))
                window.runDelimSteps(c)
            }
            // Alignment edits, ';'-separated: sel:R0,C0,R1,C1  gap  rmgap
            // slide:N  type:TEXT  del  undo  redo  up  down  strip  zoomsel
            // insert  overwrite  mafft  fasttree
            if (arg("--aln")) {
                for (const step of arg("--aln").split(";")) {
                    const [cmd, a] = step.split(":")
                    if (cmd === "sel") { const n = a.split(",").map(Number); c.select_block(n[0], n[1], n[2], n[3]) }
                    else if (cmd === "gap") c.insert_gap()
                    else if (cmd === "rmgap") c.remove_gap()
                    else if (cmd === "slide") c.slide(Number(a))
                    else if (cmd === "type") { for (const ch of a) c.key(ch.toUpperCase().charCodeAt(0), ch, 0) }
                    else if (cmd === "del") c.delete_selection()
                    else if (cmd === "undo") c.undo()
                    else if (cmd === "redo") c.redo()
                    else if (cmd === "up") c.move_rows(true)
                    else if (cmd === "down") c.move_rows(false)
                    else if (cmd === "strip") c.remove_gap_columns()
                    else if (cmd === "zoomsel") c.zoom_to_selection()
                    else if (cmd === "insert") window.alignmentInsert = true
                    else if (cmd === "overwrite") window.alignmentInsert = false
                    else if (cmd === "mafft") c.align_mafft()
                    else if (cmd === "fasttree") c.build_fasttree()
                    else if (cmd === "trimal") c.trim_alignment()
                    else if (cmd === "iqtree") c.build_iqtree()
                    else if (cmd === "distance") c.build_distance_tree(a || "k2p")
                    console.log("aln", step, "|", c.message)
                }
            }
            // Key presses through the window, ';'-separated: ctrl+=  alt+-
            // ctrl+shift+=  enter  escape  (modifiers, then a character or
            // a key's name).
            if (arg("--keys")) {
                const mods = { ctrl: Qt.ControlModifier, alt: Qt.AltModifier, shift: Qt.ShiftModifier }
                for (const k of arg("--keys").split(";")) {
                    const parts = k.split("+")
                    const ch = parts.pop() || "+"
                    let m = 0
                    for (const p of parts) m |= mods[p] || 0
                    const named = { enter: [Qt.Key_Return, "\r"], escape: [Qt.Key_Escape, ""], left: [Qt.Key_Left, ""], right: [Qt.Key_Right, ""], tab: [Qt.Key_Tab, "\t"], space: [Qt.Key_Space, " "], up: [Qt.Key_Up, ""], down: [Qt.Key_Down, ""], home: [Qt.Key_Home, ""], end: [Qt.Key_End, ""] }[ch]
                    if (named) v.pressKey(named[0], m, named[1])
                    else v.pressKey(ch.toUpperCase().charCodeAt(0), m, ch)
                    console.log("keys", k, "|", c.zoom_text, c.hscroll_size, c.vscroll_size)
                }
            }
            if (arg("--goto")) { const r = arg("--goto").split(","); c.show_range(Number(r[0]), Number(r[1])) }
            if (arg("--mode")) c.switch_view(arg("--mode"))
            if (arg("--no-sidebar")) window.sidebarOpen = false
            if (arg("--circular") && !c.circular) c.switch_view("circular")
            if (arg("--type")) {
                c.press(v.width / 2, 45, 0)
                c.release()
                for (const ch of arg("--type")) c.key(ch.toUpperCase().charCodeAt(0), ch, 0)
            }
            if (arg("--select")) {
                const r = arg("--select").split(",")
                const y = r.length > 2 ? Number(r[2]) : 45
                c.press(Number(r[0]), y, 0); c.drag(Number(r[1]), y); c.release()
            }
            if (arg("--add")) {
                const err = c.save_feature(-1, JSON.stringify({ kind: "misc_feature", location: arg("--add"), qualifiers: [{ key: "label", value: "partial test" }] }))
                if (err.length > 0) console.log("add failed:", err)
            }
            if (arg("--extract")) console.log("extracted:", c.extract_selection(), "|", c.message)
            if (arg("--hover")) {
                const h = arg("--hover").split(",")
                if (window.traceOpen) window.traceDocument.hoverAt(Number(h[0]), Number(h[1]))
                console.log("hover:", c.hover(Number(h[0]), Number(h[1])))
            }
            if (arg("--menu")) {
                const p = arg("--menu").split(",")
                menu.info = JSON.parse(c.context_press(Number(p[0]), Number(p[1])))
                menu.popup(Number(p[0]), Number(p[1]) + v.mapToItem(null, 0, 0).y)
            }
            if (arg("--menu-add")) {
                menu.info = JSON.parse(c.context_press(Number(arg("--menu-add")), 45))
                menu.popup(Number(arg("--menu-add")), 45 + v.mapToItem(null, 0, 0).y)
                menu.itemAt(5).triggered()
                menu.close()
            }
            if (arg("--ribbon")) window.ribbonTabName = window.ribbonTabs[Number(arg("--ribbon"))]
            if (arg("--genes")) console.log(window.current.predict_genes(Number(arg("--genes")), arg("--select") !== undefined))
            if (arg("--orfs")) console.log(window.current.find_orfs(Number(arg("--orfs")), arg("--select") !== undefined))
            if (arg("--activity")) {
                const files = []
                for (let i = 0; i < openFiles.count; i++) files.push(openFiles.get(i).path)
                console.log("activity:", shell.activity_log(files.join("\n"), -new Date().getTimezoneOffset()))
            }
            if (arg("--distance-action")) c.distance_action(arg("--distance-action"), arg("--distance-options") || "{}")
            if (arg("--species-dialog")) window.beginDelimitation(arg("--species-dialog"))
            if (arg("--distance-dialog")) {
                const dialog = arg("--distance-dialog") === "matrix" ? matrixDialog : arg("--distance-dialog") === "upgma" ? upgmaDialog : njDialog
                dialog.target = c
                dialog.open()
            }
            if (arg("--link-alignment")) c.link_alignment(arg("--link-alignment"))
            if (arg("--clade")) c.select_clade(JSON.stringify(arg("--clade").split(",")))
            if (arg("--export-svg")) c.export_figure(arg("--export-svg"), Number(arg("--export-scope") || 0), arg("--publication") === "1")
            if (arg("--extract-clade")) c.extract_clade_alignment()
            if (arg("--document-record")) { documentRecord.showPage(arg("--document-record") === "methods" ? 1 : 0); console.log("document-record:", c.document_record()) }
            if (arg("--tasks")) tasksDialog.open()
            if (arg("--shortcuts")) shortcuts.show(window.ribbonTab)
            if (arg("--edit")) {
                if (arg("--edit") === "new") editor.openNew(); else editor.openFeature(Number(arg("--edit")))
            }
            if (arg("--add-options-preview")) { additionOptionsPreview.start(); return }
            if (arg("--label")) { labelTyper.start(); return }
            shot.start()
        }
    }
    Timer {
        id: additionOptionsPreview
        interval: 200
        onTriggered: { docs.itemAt(tabs.currentIndex).showAdditionChoices(); shot.start() }
    }
    Timer {
        id: labelTyper
        interval: 300
        onTriggered: {
            const field = editor.valueField(0)
            if (!field) { console.log("no value field"); Qt.quit(); return }
            const keys = arg("--label").replace(/\\t/g, "\t")
            if (arg("--no-click") === undefined) {
                const p = field.mapToItem(null, field.width / 2, field.height / 2)
                window.currentView.clickAt(p.x, p.y)
            }
            window.currentView.typeText(keys)
            console.log("field text:", JSON.stringify(field.text), "focus:", field.activeFocus)
            shot.start()
        }
    }
    Timer {
        id: shot
        property bool accessibilityAudited: false
        interval: arg("--shot-delay") ? Number(arg("--shot-delay")) : 400
        onTriggered: {
            const c = window.current
            if (c && (c.busy || window.delimPending.length)) {
                if (!c.busy) window.runDelimSteps(c)
                shot.start()
                return
            }
            if (arg("--accessibility-audit") && !accessibilityAudited) {
                accessibilityAudited = true
                const view = window.currentView
                const results = {editorFocused: view.activeFocus}
                view.pressKey(Qt.Key_F6, 0, "")
                results.ribbonFocused = ribbonTabBar.currentItem.activeFocus
                view.pressKey(Qt.Key_F6, Qt.ShiftModifier, "")
                results.editorReturned = view.activeFocus
                view.pressKey(Qt.Key_T, Qt.ControlModifier | Qt.AltModifier, "t")
                Qt.callLater(() => Qt.callLater(() => {
                    results.textOpened = textRepresentation.opened
                    results.textFocused = textRepresentation.initialFocusItem.activeFocus
                    results.hasText = textRepresentation.snapshot.text.length > 0
                    view.pressKey(Qt.Key_F6, 0, "")
                    results.modalFocusKept = textRepresentation.initialFocusItem.activeFocus
                    textRepresentation.close()
                    Qt.callLater(() => {
                        results.focusRestored = view.activeFocus
                        console.log("ACCESSIBILITY_AUDIT", JSON.stringify(results))
                        shot.start()
                    })
                }))
                return
            }
            if (arg("--text-representation")) window.openTextRepresentation()
            if (arg("--ribbon-audit")) {
                const groups = []
                for (const group of ribbon.currentPage.children) {
                    if (!group.visible || group.captionY === undefined) continue
                    const point = group.mapToItem(ribbon, 0, 0)
                    groups.push({title: group.title, y: point.y, height: group.height,
                        width: group.width, implicitWidth: group.implicitWidth,
                        implicitHeight: group.implicitHeight,
                        caption: point.y + group.captionY, center: point.y + group.bodyCenterY})
                }
                console.log("RIBBON_AUDIT", JSON.stringify({page: window.ribbonTab,
                    height: ribbon.height, contentWidth: ribbon.contentWidth,
                    viewportWidth: ribbon.viewportWidth, groups: groups}))
            }
            if (arg("--render-audit") && window.currentView) {
                const view = window.currentView
                const origin = view.mapToItem(null, 0, 0)
                console.log("RENDER_AUDIT", JSON.stringify({api: view.GraphicsInfo.api,
                    software: view.GraphicsInfo.api === GraphicsInfo.Software, error: view.renderError,
                    x: origin.x, y: origin.y, width: view.width, height: view.height,
                    windowWidth: window.width, windowHeight: window.height}))
            }
            console.log("local CPU threads", window.localCpuThreads)
            console.log("interface font points", window.font.pointSize)
            if (c) console.log(typeof c.memory_report === "function" ? c.memory_report() : "Trace", "|", c.status, "|", c.hscroll_size, c.vscroll_size)
            if (window.traceOpen && c.review_mode) console.log("trace mask", c.mask_settings, "thresholds", c.review_settings, "background", c.background, "hover status", window.traceDocument.hoverStatus)
            if (arg("--window-capture") && window.currentView)
                window.currentView.saveWindow(arg("--window-capture"))
            // Capture the scene into a texture: the offscreen platform's
            // window framebuffer grab is cropped at non-unit display scales.
            let scene = window.contentItem
            while (scene.parent) scene = scene.parent
            const started = scene.grabToImage(result => {
                console.log("saved", result.saveToFile(arg("--screenshot")))
                window.closeConfirmed = true
                Qt.quit()
            })
            if (!started) { console.error("Screenshot capture failed"); Qt.exit(1) }
        }
    }
}
