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
    property bool overviewEnabled: true
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

    readonly property real ribbonSizeFactor: preferenceValues.ui && preferenceValues.ui.ribbon_size_percent !== undefined
        ? preferenceValues.ui.ribbon_size_percent / 100 : 1
    // Read by AppCheckBox and AppSpinBox everywhere in the window.
    readonly property int tickBoxSize: preferenceValues.ui && preferenceValues.ui.tick_box_size ? preferenceValues.ui.tick_box_size : 14
    readonly property int inputBoxHeight: preferenceValues.ui && preferenceValues.ui.input_box_height ? preferenceValues.ui.input_box_height : 20

    // Closing the window (or Ctrl+Q) asks about unsaved changes first.
    property bool closeConfirmed: false
    property bool discardAuthoredOnQuit: false
    function discardEmptyDrafts() {
        for (let i = 0; i < openFiles.count; i++) discardEmptyDraft(window.documentAt(i))
    }
    onClosing: (close) => {
        if (closeConfirmed) { discardEmptyDrafts(); documentOpens.shutdown(); return }
        if (!discardAuthoredOnQuit && documentRecord.unsavedDrafts().length) {
            close.accepted = false
            draftQuitDialog.open()
            return
        }
        const unsaved = window.unsavedTabs()
        if (unsaved.length === 0) { discardEmptyDrafts(); documentOpens.shutdown(); return }
        close.accepted = false
        quitDialog.ask(unsaved)
    }
    function unsavedTabs() {
        const out = []
        for (let i = 0; i < openFiles.count; i++) {
            const d = window.documentAt(i)
            if (d && d.controller.dirty) out.push(documentTarget(i))
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

    property alias ribbonNavigation: ribbonHints
    RibbonKeyHints {
        id: ribbonHints
        parent: window.contentItem
        anchors.fill: parent
        z: 1000
        hostWindow: window
        tabBar: ribbonTabBar
        ribbon: ribbon
    }
    // Open files, one tab each.
    ListModel { id: openFiles }
    function documentAt(index) {
        if (index < 0 || index >= openFiles.count) return null
        const slot = docs.itemAt(index)
        return slot && slot.adoptedSlot === true ? (slot.ready ? slot.documentView : null) : slot
    }
    // Consent belongs to a tab lifetime and owner, never its shifting row.
    function documentTarget(index) {
        const document = documentAt(index)
        if (!document) return null
        const row = openFiles.get(index)
        return {token: row.openToken, controller: document.controller, path: row.path}
    }
    function targetIndex(target) {
        if (!target || !target.controller) return -1
        const index = requestIndex(target.token)
        const document = documentAt(index)
        return document && document.controller === target.controller && openFiles.get(index).path === target.path ? index : -1
    }
    function appendLegacy(row) {
        row.openToken = documentOpens.track(row.path)
        openFiles.append(row)
    }
    function removeDocument(index) {
        if (index < 0 || index >= openFiles.count) return
        documentOpens.retire(openFiles.get(index).openToken || "")
        const applying = applyingOpenFocus
        applyingOpenFocus = true
        openFiles.remove(index)
        applyingOpenFocus = applying
    }
    function requestIndex(token) {
        for (let i = 0; i < openFiles.count; ++i)
            if (openFiles.get(i).openToken === token) return i
        return -1
    }
    property bool applyingOpenFocus: false
    onActiveFocusItemChanged: if (!applyingOpenFocus) documentOpens.revoke_focus()
    Component.onDestruction: documentOpens.shutdown()
    DocumentOpens {
        id: documentOpens
        onAcquired: (token, kind) => {
            const index = window.requestIndex(token)
            const slot = index >= 0 ? docs.itemAt(index) : null
            if (slot && slot.adoptedSlot === true) slot.construct(kind)
            else documentOpens.retire(token)
        }
        onFailed: (token, message) => {
            const index = window.requestIndex(token)
            if (index < 0) return
            const path = openFiles.get(index).path
            window.removeDocument(index)
            provenance.notification(path, message, "fail")
        }
        onActivated: (token, review, installedPath) => {
            const index = window.requestIndex(token)
            const document = index >= 0 ? window.documentAt(index) : null
            if (!document || !document.controller || document.controller.file_path !== installedPath) return
            window.applyingOpenFocus = true
            tabs.currentIndex = index
            window.focusOpenedDocument = true
            window.refreshCurrent()
            if (review && window.current === document.controller && window.currentPath === document.path
                    && documentRecord.controller === document.controller && documentRecord.documentPath === document.path)
                documentRecord.showPage(2)
            window.applyingOpenFocus = false
        }
    }

    // The active tab's controller and view, or null.
    property var currentDocument: null
    property bool projectingDocument: false
    property var current: null
    property var currentView: null
    readonly property var traceDocument: traceOpen ? currentDocument : null
    readonly property var alignmentDocument: alignmentOpen ? currentDocument : null
    readonly property var speciesView: delimitationOpen ? currentDocument : null
    // The species view's display tree, while it shows one.
    readonly property var speciesTree: speciesView !== null && current.display_path.length > 0 ? speciesView.treeController : null
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
    readonly property var ribbonTabs: ["File", "View"].concat(
        currentKind === "sequence" ? ["Sequence"] : alignmentOpen ? ["Alignment", "Phylogeny", "Delimitation"]
        : treeOpen ? ["Tree", "Delimitation"] : delimitationOpen ? ["Delimitation"] : traceOpen ? ["Trace"] : [], ["Export"])
    onRibbonTabsChanged: {
        if (projectingDocument) return
        if (!ribbonTabs.includes(ribbonTabName)) ribbonTabName = ribbonTabs.length > 3 ? ribbonTabs[2] : "File"
    }
    readonly property string ribbonTab: ribbonTabs.includes(ribbonTabName) ? ribbonTabName : "File"
    // Each tab's panel in the ribbon's stack.
    readonly property var ribbonPanels: ({ "File": 0, "View": 1, "Sequence": 2, "Alignment": 3, "Tree": 4, "Phylogeny": 5, "Delimitation": 6, "Trace": 7, "Export": 8 })
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
    readonly property var applicationCommands: [findCommand, newSequenceCommand, addAnnotationCommand, openCommand, saveCommand, saveAsCommand, projectCommand, shortcutsCommand, aboutCommand, preferencesCommand, textRepresentationCommand, contextShortcutsCommand, learnCommand, exportSvgCommand, exportPngCommand, historyCommand, methodsCommand, showProvenanceCommand, verifyProvenanceCommand, packageCommand, recoverCommand, installedOperationsCommand].concat(ribbonCommands)
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
        const result = JSON.parse(preferences.remember_project(path))
        if (result.error) {
            recentProjectsError = result.error
            return
        }
        shell.project_dir = path
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
        if (currentPath.startsWith("untitled:")) return shell.project_dir.length ? shell.local_file_url(shell.project_dir) : StandardPaths.writableLocation(StandardPaths.DocumentsLocation)
        const path = traceOpen && current && current.export_path ? current.export_path : current && current.file_path ? current.file_path : currentPath
        return path.length ? shell.parent_folder_url(path) : shell.local_file_url(shell.project_dir)
    }
    function projectPickerFolder() {
        const home = window.localPath(StandardPaths.writableLocation(StandardPaths.HomeLocation))
        return shell.local_file_url(preferences.project_picker_directory(home))
    }
    property var projectDialog: null
    function openProjectPicker() {
        if (projectDialog && projectDialog.visible) return
        // Qt applies FolderDialog.currentFolder to the backend on its first
        // show only. A fresh native dialog makes every opening use our policy.
        projectDialog = projectDialogComponent.createObject(window, {currentFolder: projectPickerFolder()})
        if (projectDialog) projectDialog.open()
    }

    function showActivity() { tasksDialog.open() }
    function localPath(url) { return shell.local_file_path(url) }

    property bool focusOpenedDocument: false

    function refreshCurrent(allowFocus = true) {
        const d = window.documentAt(tabs.currentIndex)
        // Disable typed bindings while publishing the new projection. A tree
        // must never reach a sequence-only binding under the preceding kind.
        projectingDocument = true
        currentKind = ""
        currentDocument = d
        current = d ? d.controller : null
        currentView = d ? d.view : null
        currentPath = d ? d.path : ""
        currentKind = d ? d.kind : ""
        projectingDocument = false
        if (!ribbonTabs.includes(ribbonTabName)) ribbonTabName = ribbonTabs.length > 3 ? ribbonTabs[2] : "File"
        if (allowFocus && currentView && (focusOpenedDocument || !tabs.activeFocus)) {
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
        const regions = [ribbonTabBar, tree, tabs, delimitationOpen ? window.documentAt(tabs.currentIndex) : currentView].filter(item => item && item.visible && item.enabled)
        if (!regions.length) return
        let active = -1
        for (let i = 0; i < regions.length; ++i) {
            for (let item = window.activeFocusItem; item; item = item.parent) {
                if (item === regions[i] || (item.objectName === "documentOverview" && item.documentItem === regions[i])) { active = i; break }
            }
        }
        if (active < 0) return
        const region = regions[(active + (backward ? regions.length - 1 : 1)) % regions.length]
        const target = region === ribbonTabBar || region === tabs ? region.currentItem : region
        if (target) target.forceActiveFocus(Qt.TabFocusReason)
        const focused = window.activeFocusItem
        if (focused && focused.keyboardFocus !== undefined) focused.keyboardFocus = true
    }
    Shortcut { sequence: "F6"; onActivated: window.focusRegion(false) }
    Shortcut { sequence: "Shift+F6"; onActivated: window.focusRegion(true) }

    // Open a file in a tab, or switch to its tab if it's already open.
    function openPath(path, reviewMethods = false) {
        if (documentOpens.eligible(path)) {
            const existing = documentOpens.existing(path)
            if (!existing.length && !shell.project_dir.length) shell.use_project_of(path)
            const token = documentOpens.request(path, shell.project_dir, reviewMethods)
            if (!token.length) { provenance.notification(path, "Could not allocate an open request", "fail"); return }
            if (existing.length) {
                applyingOpenFocus = true
                tabs.currentIndex = requestIndex(token)
                refreshCurrent(false)
                applyingOpenFocus = false
                documentOpens.activate(token)
                return
            }
            applyingOpenFocus = true
            openFiles.append({path: path, dirty: false, kind: "canvas", openToken: token})
            tabs.currentIndex = openFiles.count - 1
            applyingOpenFocus = false
            documentOpens.start(token)
            return
        }
        documentOpens.revoke_focus()
        const kind = shell.file_kind(path)
        // The specimen table is project data, edited on the Export tab.
        if (kind === "metadata") {
            ribbonTabName = "Export"
            specimenTable.open()
            return
        }
        // Submissions are project documents edited on the Export tab.
        if (kind === "submission") {
            ribbonTabName = "Export"
            window.openSubmission(path)
            return
        }
        focusOpenedDocument = true
        for (let i = 0; i < openFiles.count; i++) {
            if (openFiles.get(i).path === path) {
                tabs.currentIndex = i
                Qt.callLater(refreshCurrent)
                if (reviewMethods) reviewLegacyDocument(i, path)
                return
            }
        }
        if (!shell.project_dir.length) shell.use_project_of(path)
        window.appendLegacy({ path: path, dirty: false, kind: kind })
        tabs.currentIndex = openFiles.count - 1
        if (reviewMethods) reviewLegacyDocument(tabs.currentIndex, path)
    }
    function reviewLegacyDocument(index, path) {
        const document = window.documentAt(index)
        const token = documentOpens.request(path, shell.project_dir, true)
        // Legacy trace acquisition retains its existing deferred completion.
        // The continuation still checks its original view/token and actual owner.
        Qt.callLater(() => {
            const row = requestIndex(token)
            if (row < 0 || window.documentAt(row) !== document || !document || !document.controller.file_path.length) return
            documentOpens.path_changed(token, path, document.controller.file_path)
            documentOpens.activate(token)
        })
    }

    property var delimitationSource: null
    property var delimitationDocument: null
    property var delimitationRequest: null
    function documentIsOpen(document, controller, path) {
        if (!document || !controller || controller.file_path !== path) return false
        for (let i = 0; i < openFiles.count; ++i)
            if (window.documentAt(i) === document && document.controller === controller) return true
        return false
    }
    function retireDelimitationDocument(document) {
        if (delimitationDocument === document) {
            delimitationSource = null
            delimitationDocument = null
        }
        if (delimitationRequest && delimitationRequest.document === document)
            delimitationRequest = null
    }
    function beginDelimitation(method) {
        if (delimitationRequest || !current || current.busy || !(alignmentOpen || treeOpen || delimitationOpen)) return
        if (method === "asap" && !alignmentOpen && !(delimitationOpen && current.linked_alignment.length)) return
        delimitationSource = current
        delimitationDocument = window.documentAt(tabs.currentIndex)
        speciesDialog.engineProblems = method === "ptp" ? JSON.parse(shell.delimitation_engines()) : ({})
        speciesDialog.show(method, alignmentOpen ? current.file_path : current.linked_alignment || "",
            treeOpen ? current.file_path : delimitationOpen ? current.display_path : "")
    }
    function requestDelimitation(method, input, rooted, settings, displayMethod) {
        const source = delimitationSource
        const document = delimitationDocument
        if (delimitationRequest || !source || !document || source.busy || !documentIsOpen(document, source, document.path)) return
        if (document.kind === "delimitation") {
            document.startAnalysis(method, input, rooted, settings, displayMethod)
            return
        }
        if (document.kind !== "alignment" && document.kind !== "tree") return
        // Only request context lives here. The backend owns the frozen input,
        // execution state and created workspace, including failure diagnostics.
        delimitationRequest = {source: source, document: document, path: source.file_path,
            method: method, input: input, rooted: rooted, settings: settings, displayMethod: displayMethod,
            usesOpenAlignment: document.kind === "alignment" && input === source.file_path}
        if (!source.create_delimitation(method)) delimitationRequest = null
    }
    function completeDelimitation(request, path) {
        if (!request || request !== delimitationRequest) return
        delimitationRequest = null
        if (!path.length || !documentIsOpen(request.document, request.source, request.path)) return
        // Never reuse or overwrite an already opened result (including edits).
        for (let i = 0; i < openFiles.count; ++i)
            if (openFiles.get(i).path === path) return
        window.appendLegacy({path: path, dirty: false, kind: "delimitation"})
        const document = window.documentAt(openFiles.count - 1)
        Qt.callLater(() => {
            if (!documentIsOpen(request.document, request.source, request.path) ||
                    !document || !documentIsOpen(document, document.controller, path)) return
            document.draft = true
            document.startAnalysis(request.method,
                request.usesOpenAlignment ? document.controller.linked_alignment : request.input,
                request.rooted, request.settings, request.displayMethod)
        })
    }
    Connections {
        target: window.delimitationRequest ? window.delimitationRequest.source : null
        function onWorkspaceCreated(path) { window.completeDelimitation(window.delimitationRequest, path) }
    }
    DelimitationAnalysisDialog {
        id: speciesDialog
        pathResolver: shell
        workers: window.localCpuThreads
        onRequested: (method, input, rooted, settings, displayMethod) =>
            window.requestDelimitation(method, input, rooted, settings, displayMethod)
    }
    DelimitationController { id: combineController }
    FileDialog {
        id: combinePicker
        title: "Combine saved species hypotheses"
        fileMode: FileDialog.OpenFiles
        nameFilters: ["Species hypotheses (*.spart *.spartxml *.spart-xml *.xml)"]
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        readonly property var paths: Array.from(selectedFiles, url => window.localPath(url))
        onAccepted: {
            if (selectedFiles.length < 2) { combineError.message = "Choose at least two species hypothesis files."; combineError.open(); return }
            const candidates = JSON.parse(combineController.combine_candidates(JSON.stringify(paths)))
            if (!candidates.length) { combineError.message = combineController.message; combineError.open(); return }
            combineChoice.show(candidates)
        }
    }
    CombineHypothesesDialog {
        id: combineChoice
        property string selection: ""
        onChosenHypotheses: chosen => {
            selection = chosen
            // Suggest <shared root>.combinedNN.spart beside the files just chosen.
            const paths = combinePicker.paths
            const suggested = combineController.combine_name(JSON.stringify(paths))
            combinedDestination.currentFolder = shell.parent_folder_url(suggested.length ? suggested : paths[paths.length - 1])
            if (suggested.length) combinedDestination.selectedFile = shell.local_file_url(suggested)
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
            const output = window.localPath(selectedFile)
            if (combineController.combine_files(JSON.stringify(combinePicker.paths), output, combineChoice.selection)) window.openPath(output)
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
            const existing = window.documentAt(i)
            if (openFiles.get(i).path !== paths[0] || !existing) continue
            if (existing.kind === "trace" && existing.working) { tabs.currentIndex = i; return }
            if (existing.controller.dirty) preserve = true
        }
        if (preserve) {
            window.appendLegacy({ path: paths[0], dirty: false, kind: "trace" })
            tabs.currentIndex = openFiles.count - 1
        } else window.openPath(paths[0])
        const traceDocument = window.documentAt(tabs.currentIndex)
        Qt.callLater(() => {
            if (!traceDocument) return
            traceDocument.startReview(paths, reference, target, cutoff)
        })
    }

    Connections {
        target: window.current
        ignoreUnknownSignals: true
        function onTaxaSelected(names) {
            if (!window.current) return
            const source = (window.treeOpen || window.delimitationOpen) ? window.current.linked_alignment : window.current.file_path
            if (!source) return
            for (let i = 0; i < docs.count; i++) {
                const d = window.documentAt(i)
                if (!d || d.controller === window.current) continue
                if ((window.treeOpen || window.delimitationOpen) && d.kind === "alignment" && d.controller.file_path === source)
                    d.controller.highlight_taxa(names)
                else if (window.alignmentOpen && (d.kind === "tree" || d.kind === "delimitation") && d.controller.linked_alignment === source)
                    d.controller.highlight_taxa(names)
            }
        }
    }

    // A species workspace created for an analysis that produced nothing is
    // removed rather than left as an empty file beside the data.
    function discardEmptyDraft(d) {
        if (d && d.kind === "delimitation" && d.draft) d.controller.discard_empty_workspace()
    }
    function closeTab(i) {
        documentOpens.revoke_focus()
        const d = window.documentAt(i)
        if (d && d.controller.dirty) { confirmClose.target = documentTarget(i); confirmClose.saveError = ""; confirmClose.open(); return }
        discardEmptyDraft(d)
        window.removeDocument(i)
        Qt.callLater(refreshCurrent)
    }

    // Clipboard access goes through a hidden text edit.
    TextEdit {
        id: clipboard
        visible: false
        function put(text) { clipboard.text = text; clipboard.selectAll(); clipboard.copy() }
    }

    AppCommand {
        id: installedOperationsCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "tools.installed-operations"
        text: "Installed operations…"
        iconName: "more"
        tooltip: "Choose a compatible installed operation; other schemas remain available through the API"
        enabled: window.current !== null && window.currentPath.length > 0
        onTriggered: operationPicker.show(shell.project_dir.length ? shell.project_dir : shell.local_file_path(shell.parent_folder_url(window.currentPath)), window.currentPath, !!window.current.dirty)
    }
    AppCommand {
        id: newSequenceCommand
        customization: window.customization
        commandId: "file.new-sequence"
        text: "New"
        tooltip: "New sequence"
        iconName: "new-file"
        defaultShortcut: "Ctrl+N"
        onTriggered: newSequenceDialog.prepare("", "")
    }
    AppCommand {
        id: addAnnotationCommand
        customization: window.customization
        commandId: "ribbon.annotate-the-selection"
        text: "Add annotation"
        tooltip: "Annotate the selection; right-to-left selection defaults to reverse strand"
        iconName: "annotation-add"
        defaultShortcut: "Ctrl+Shift+N"
        enabled: window.currentKind === "sequence" && window.current !== null && !window.current.busy
            && !(window.activeFocusItem && window.activeFocusItem.cursorPosition !== undefined)
        onTriggered: editor.openNew()
    }
    property var pendingSequence: null
    property int nextSequence: 0
    AddSequenceDialog {
        id: newSequenceDialog
        controller: shell
        newDocument: true
        onInputError: message => validationMessage = message
        onRejected: {
            const pending = window.pendingSequence
            window.pendingSequence = null
            if (!pending) return
            for (let i = 0; i < openFiles.count; ++i) {
                if (openFiles.get(i).path === pending.path) { window.removeDocument(i); break }
            }
            tabs.currentIndex = pending.previous
        }
        onCreateRequested: (name, sequence) => {
            if (window.pendingSequence) return
            const error = shell.new_sequence_error(name, sequence)
            if (error.length) { validationMessage = error; return }
            const path = "untitled:" + (++window.nextSequence) + "/" + name.trim() + ".gb"
            window.pendingSequence = {path: path, name: name, sequence: sequence, previous: tabs.currentIndex}
            window.appendLegacy({path: path, dirty: false, kind: "sequence"})
            tabs.currentIndex = openFiles.count - 1
        }
    }
    function initializeSequence(document) {
        const pending = window.pendingSequence
        if (!pending || pending.path !== document.path) return
        const error = document.controller.create_sequence(pending.name, pending.sequence, shell.project_dir)
        window.pendingSequence = null
        if (error.length) {
            window.removeDocument(document.index)
            tabs.currentIndex = pending.previous
            newSequenceDialog.validationMessage = error
            return
        }
        newSequenceDialog.accept()
        Qt.callLater(() => { window.refreshCurrent(); document.view.forceActiveFocus() })
    }
    AppCommand {
        id: openCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "file.open"
        text: "Open…"
        iconName: "open"
        tooltip: "Open a sequence, alignment, trace, tree, review, species workspace, specimen table or submission"
        defaultShortcut: "Ctrl+O"
        onTriggered: openDialog.open()
    }
    AppCommand {
        id: saveCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "file.save"
        text: "Save"
        iconName: "save"
        tooltip: "Save the current file or trace review"
        defaultShortcut: "Ctrl+S"
        enabled: window.current !== null && (!window.traceOpen || window.current.review_mode)
        onTriggered: window.traceOpen ? window.traceDocument.saveReview() : window.saveDocument(tabs.currentIndex, "", null)
    }
    AppCommand {
        id: saveAsCommand
        disabledReason: window.disabledReasonFor(commandId)
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
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "project.open"
        text: "Open project…"
        iconName: "project"
        tooltip: "Choose or create a project folder"
        onTriggered: window.openProjectPicker()
    }
    AppCommand {
        id: shortcutsCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "help.shortcuts"
        text: "Keyboard shortcuts…"
        iconName: "keyboard"
        tooltip: "List all keyboard shortcuts"
        onTriggered: shortcuts.show("")
    }
    AppCommand {
        id: aboutCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "help.about"
        text: "About Basecraft…"
        iconName: "info"
        tooltip: "Version and licence"
        onTriggered: about.open()
    }

    AppCommand {
        id: preferencesCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "application.preferences"
        text: "Preferences…"
        iconName: "settings"
        tooltip: "Appearance, files, compute, ribbon and shortcuts"
        onTriggered: preferencesDialog.open()
    }
    AppCommand {
        id: findCommand
        customization: window.customization
        commandId: "view.find"
        text: "Find"
        iconName: "search"
        tooltip: "Find names or sequence motifs in this document"
        defaultShortcut: "Ctrl+F"
        enabled: window.current !== null
        onTriggered: {
            const document = window.documentAt(tabs.currentIndex)
            if (document && document.openFind) document.openFind()
        }
    }
    AppCommand {
        id: textRepresentationCommand
        disabledReason: window.disabledReasonFor(commandId)
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
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "help.context-shortcuts"
        text: "Shortcuts…"
        iconName: "keyboard"
        tooltip: "Keyboard shortcuts for this tab"
        onTriggered: shortcuts.show(window.ribbonTab)
    }
    AppCommand {
        id: learnCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "help.learn"
        text: "Learn"
        iconName: "book"
        tooltip: "Manual pages are not written yet"
    }
    AppCommand {
        id: exportSvgCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "ribbon.export-an-editable-svg-of-this-view"
        text: "Export SVG…"
        iconName: "save-as"
        tooltip: "Export an editable SVG of this view"
        enabled: window.current !== null
        onTriggered: window.exportSvg()
    }
    AppCommand {
        id: exportPngCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "figure.export-png"
        text: "Export PNG"
        iconName: "figure-export"
        tooltip: "Export a PNG image of this view"
        enabled: window.current !== null
        onTriggered: window.exportSvg("png")
    }
    AppProvenance {
        id: provenance
        onMessageChanged: if (message.length && message !== "notify: Working…") notification("", message, message.startsWith("fail:") ? "fail" : "done")
    }
    property var activeTasks: ({jobs: []})
    readonly property int runningTaskCount: (activeTasks.jobs || []).filter(job => ["queued", "running", "cancelling", "finalizing"].includes(job.status)).length
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
    readonly property string savedRecordPath: window.traceOpen && window.current ? window.current.file_path : window.currentPath
    property string provenanceSource: ""
    property string provenanceExport: ""
    function inspectProvenance(operation) {
        provenanceExport = ""
        provenanceDialog.open()
        provenance.start(operation, window.savedRecordPath, "")
    }
    function exportProvenance(operation) {
        provenanceSource = window.savedRecordPath
        provenanceExport = operation
        provenanceFolder.open()
    }
    AppCommand {
        id: historyCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "provenance.history"
        text: "Provenance"
        iconName: "history"
        tooltip: "Sources, analyses and edits for the current document"
        enabled: window.current !== null && !provenance.busy
        onTriggered: documentRecord.showPage(0)
    }
    AppCommand {
        id: methodsCommand
        disabledReason: tooltip
        customization: window.customization
        commandId: "provenance.methods"
        text: "Methods"
        iconName: "methods-export"
        tooltip: window.current === null ? "Open a document to read its methods" : provenance.busy ? "Wait for the current provenance operation" : "Read methods for the current document, including unsaved changes"
        enabled: window.current !== null && !provenance.busy
        onTriggered: { documentRecord.savedRevision = false; documentRecord.showPage(1) }
    }
    AppCommand {
        id: showProvenanceCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "provenance.show-files"
        text: "Record folder"
        iconName: "open"
        tooltip: "Open the saved result's readable provenance folder"
        enabled: window.current !== null && !provenance.busy
        onTriggered: {
            const folder = provenance.bundle_path(window.savedRecordPath)
            if (folder.length) Qt.openUrlExternally(shell.local_file_url(folder))
            else provenanceDialog.open()
        }
    }
    AppCommand {
        id: verifyProvenanceCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "provenance.verify"
        text: "Verify…"
        tooltip: "Verify recorded input and output hashes without executing tools"
        enabled: window.current !== null && !provenance.busy
        onTriggered: window.inspectProvenance("verify")
    }
    AppCommand {
        id: packageCommand
        disabledReason: window.disabledReasonFor(commandId)
        customization: window.customization
        commandId: "provenance.package"
        text: "Package for reproduction…"
        tooltip: "Copy the result and retained history to a new package directory"
        enabled: window.current !== null && !provenance.busy
        onTriggered: window.exportProvenance("package")
    }
    AppCommand {
        id: recoverCommand
        disabledReason: window.disabledReasonFor(commandId)
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
            AppMessageBar {
                Layout.fillWidth: true
                busy: provenance.busy
                workingText: provenance.message || "Working on provenance"
                message: provenance.message
                cancellable: false
            }
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
        const specific = {
            "ribbon.copy-the-selected-annotation-s-sequence": "clipboard",
            "ribbon.save-the-selection-as-a-new-file": "selection",
            "ribbon.rename-the-cursor-s-row": "rename",
            "ribbon.annotate-open-reading-frames": "gene-orf",
            "ribbon.annotate-protein-coding-genes-with-prodigal": "gene-predict",
            "trace.compute-calls": "trace-compute",
            "trace.cancel-calls": "trace-cancel",
            "ribbon.combine-up-to-three-reads": "trace-draft",
            "ribbon.choose-the-consensus-range-to-keep-in-exports": "trace-mask",
            "ribbon.find-partial-terminal-primer-candidates-and-mask-them": "trace-primer",
            "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft": "trace-cutoff",
            "ribbon.choose-alignment-context": "link",
            "ribbon.add-support-values-from-another-analysis-of-the-same": "support-add"
}
        if (specific[id]) return specific[id]
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

    // Presentation only: availability stays with each existing command/control.
    function disabledReasonFor(id) {
        if (id.startsWith("metadata.") || id.startsWith("submission.")) {
            if (!shell.project_dir.length) return "Choose a project folder with File > Project to use project data"
            if (["metadata.import-headers", "metadata.template"].includes(id)) return "Open a FASTA file to use its records"
            if (id === "submission.new") return "Open a marker alignment to start a submission"
            if (submissionJobs.busy) return "Wait for submission validation to finish"
            return "Start or open a GenBank submission in this project"
        }
        if (id.startsWith("species.")) {
            if (id.startsWith("species.asap-") && !alignmentOpen && !(current && current.linked_alignment)) return "Open an alignment or link one to the species workspace to run ASAP"
            if (/^species\.(gmyc|ptp|manual|asap)-/.test(id)) return current && current.busy ? "Wait for the current analysis to finish" : "Open an alignment, tree or species workspace to delimit species"
            if (id.startsWith("species.combine-")) return "Open a document, then choose species workspaces to combine"
            if (!speciesView) return "Open a species delimitation workspace to use this command"
            if (current.busy) return "Wait for the workspace operation to finish"
            if (id === "species.roster-links") return "Choose a project folder and open its specimen metadata table to link records"
            if (id === "species.threshold-cut") return "Attach an ultrametric display tree to enable threshold selection"
            if (id === "species.display") return "A display tree is already attached; use the workspace's tree view"
            if (id === "species.layout") return "Choose at least one hypothesis in Columns to lay out its bars"
            if (id === "species.support-style") return "Attach a display tree with support values to style support"
            return "Attach a display tree to the species workspace to show it"
        }
        if (id === "help.learn") return "Manual pages are not written yet"
        if (id === "file.recover-work") return "Wait for the current provenance operation to finish"
        if (!current) return "Open a document to use this command"
        if ((id.startsWith("provenance.") || id === "file.recover-work") && provenance.busy) return "Wait for the current provenance operation to finish"
        if (id === "trace.cancel-calls") return "Compute alternative trace calls before cancelling them"
        if (traceOpen && traceDocument && traceDocument.working) return "Wait for trace processing to finish"
        if (current.busy || current.calling) return "Wait for the current operation to finish"
        if (traceOpen && !current.review_mode) return "Open a Sanger review to edit or export its reviewed consensus"
        if (id === "trace.compute-calls" || id === "ribbon.combine-up-to-three-reads") return "Open a single-read Sanger review to compute calls or draft additional reads"
        if (id === "ribbon.choose-alignment-context") return "This review already has alignment context; open a standalone review to choose context"
        if (id.includes("edited-consensus")) {
            if (current.unresolved > 0) return "Resolve or accept every remaining review issue before exporting the consensus"
            return "Choose alignment context for this review before adding its consensus"
        }
        if (id.includes("delete-columns-containing-only-gaps")) return "This alignment has no all-gap columns to delete"
        if (id.includes("which-support-values")) return "Use Support > Add to add tree support values before styling them"
        if (id === "ribbon.zoom-to-the-selection") return "Open a sequence or alignment and select a region to zoom to it"
        if (id === "tree.rate-smoothing") return "Save the tree to a file before smoothing its rates"
        if (id === "file.save-as" && traceOpen) return "Open a Sanger review to save review decisions; a raw trace cannot be saved as a new document"
        if (!currentPath.length) return "Save the document to a file before using this command"
        return "Open a compatible document to use this command"
    }

    component RibbonButton: AppCommandButton {
        customization: window.customization
        disabledReason: window.disabledReasonFor(commandId)
        enabled: window.current !== null
        iconName: window.ribbonIconFor(commandId)
        Component.onCompleted: window.registerRibbonCommand(commandAction)
    }
    // The same last group on every ribbon tab.
    // A ribbon button that shows whether its mode is on.
    component ExportConsensusButton: RibbonButton {
        id: consensusExportButton
        // The FASTA motif fits the shared compact symbol band.
        symbolSize: 16
        Accessible.name: tip
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
        readonly property bool canRun: (window.alignmentOpen || window.treeOpen || window.delimitationOpen) && window.current !== null && !window.current.busy
        RibbonButton { commandId: "species.gmyc-" + speciesMethods.commandSuffix; text: "GMYC"; iconName: "tree-gmyc"; enabled: speciesMethods.canRun; onInvoked: window.beginDelimitation("gmyc") }
        RibbonButton { commandId: "species.asap-" + speciesMethods.commandSuffix; text: "ASAP"; iconName: "matrix-asap"; enabled: (window.alignmentOpen || (window.delimitationOpen && window.current && window.current.linked_alignment.length > 0)) && speciesMethods.canRun; onInvoked: window.beginDelimitation("asap") }
        RibbonButton { commandId: "species.ptp-" + speciesMethods.commandSuffix; text: "PTP"; iconName: "phylogram-ptp"; enabled: speciesMethods.canRun; onInvoked: window.beginDelimitation("ptp") }
        RibbonButton { commandId: "species.manual-" + speciesMethods.commandSuffix; text: "Manual"; iconName: "hypothesis-manual"; enabled: speciesMethods.canRun; onInvoked: window.beginDelimitation("manual") }
        RibbonButton { commandId: "species.combine-" + speciesMethods.commandSuffix; text: "Combine"; iconName: "hypothesis-combine"; onInvoked: combinePicker.open() }
    }
    component HelpGroup: RibbonHelpGroup {
        methodsAction: methodsCommand
        shortcutsAction: contextShortcutsCommand
        aboutAction: aboutCommand
        learnAction: learnCommand
        preferencesAction: preferencesCommand
        exportSvgAction: exportSvgCommand
        exportPngAction: exportPngCommand
        backgroundColor: shell.background
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
                    ribbonSizeFactor: window.ribbonSizeFactor
                    Layout.fillWidth: true
                    onEnterRibbon: backward => ribbon.enter(backward)
                    onEscapeRequested: if (window.currentView) window.currentView.forceActiveFocus()
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
                    // A multi-line error must never grow the top bar.
                    maximumLineCount: 1
                    text: window.current ? String(window.current.message || "").split("\n")[0] : ""
                    // Green for done, red for failed, plain for hints.
                    color: !window.current ? shell.foreground
                        : window.current.message_kind === "ok" ? window.current.ok_color
                        : window.current.message_kind === "error" ? window.current.error_color
                        : shell.foreground
                    // A message goes away by itself after 10 seconds.
                    Timer {
                        id: messageTimer
                        interval: 10000
                        property var sourceController: null
                        onTriggered: {
                            if (sourceController && sourceController === window.current && !sourceController.busy)
                                sourceController.message = ""
                        }
                    }
                    onTextChanged: if (messageLabel.text.length > 0) {
                        messageTimer.sourceController = window.current
                        messageTimer.restart()
                    }
                }
                AppButton {
                    text: window.runningTaskCount ? "Tasks (" + window.runningTaskCount + ")" : "Tasks" + (window.failedTaskCount ? " · " + window.failedTaskCount + " failed" : "")
                    Accessible.name: "Tasks and notifications, " + window.runningTaskCount + " active, " + window.failedTaskCount + " failed"
                    onClicked: tasksDialog.open()
                }
            }
        }

        Ribbon {
            id: ribbon
            ribbonSizeFactor: window.ribbonSizeFactor
            objectName: "applicationRibbon"
            onEscapeRequested: if (window.currentView) window.currentView.forceActiveFocus()
            // Keep the same compact symbol band on every page.
            symbolBandSize: 16
            Layout.fillWidth: true
            currentIndex: window.ribbonPanels[window.ribbonTab]
            background: Rectangle { color: Qt.alpha(shell.foreground, 0.025) }

                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        title: "Project"
                        AppRibbonButton { action: projectCommand; visible: projectCommand.visible; text: "Open/New"; iconName: "project"; tip: projectCommand.effectiveTooltip }
                        AppRibbonButton {
                            id: recentProjectButton
                            property string keyHint: "RC"
                            text: "Recent"
                            iconName: "project"
                            tip: "Open a recent project"
                            Accessible.role: Accessible.ButtonMenu
                            onClicked: recentProjectMenu.popup(recentProjectButton, 0, height)
                            RibbonMenu {
                                id: recentProjectMenu
                                objectName: "recentProjectMenu"
                                sizeFactor: window.ribbonSizeFactor
                                RibbonMenuItem { sizeFactor: window.ribbonSizeFactor; text: "No recent projects"; enabled: false; visible: window.recentProjects.length === 0 }
                                Repeater {
                                    model: window.recentProjects
                                    RibbonMenuItem { sizeFactor: window.ribbonSizeFactor;
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
                        AppRibbonButton { action: newSequenceCommand; visible: newSequenceCommand.visible; iconName: "new-file"; tip: newSequenceCommand.effectiveTooltip }
                        AppRibbonButton { action: saveCommand; visible: saveCommand.visible; iconName: "save"; tip: saveCommand.effectiveTooltip }
                        AppRibbonButton { action: saveAsCommand; visible: saveAsCommand.visible; iconName: "save-as"; tip: saveAsCommand.effectiveTooltip }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Species workspaces"
                        RibbonButton { commandId: "species.combine-project"; text: "Combine"; iconName: "hypothesis-combine"; enabled: true; tip: "Combine saved species hypotheses"; onInvoked: combinePicker.open() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup { title: "Document record"; actions: [historyCommand, methodsCommand] }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Recovery"
                        AppRibbonButton { action: recoverCommand; visible: recoverCommand.visible; iconName: "history"; tip: recoverCommand.effectiveTooltip }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }

                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        title: "Zoom"
                        disabledReason: window.delimitationOpen ? "Attach a display tree to the species workspace to zoom it" : "Open a document to zoom its view"
                        enabled: (window.delimitationOpen ? window.speciesTree : window.current) !== null
                        RibbonButton { commandId: "ribbon.zoom-out"; iconName: "zoom-out"; tip: "Zoom out"; keys: "Ctrl+−"; onInvoked: (window.delimitationOpen ? window.speciesTree : window.current).zoom_out() }
                        RibbonButton { commandId: "ribbon.zoom-in"; iconName: "zoom-in"; tip: "Zoom in"; keys: "Ctrl++"; onInvoked: (window.delimitationOpen ? window.speciesTree : window.current).zoom_in() }
                        RibbonButton { commandId: "ribbon.fit";
                            text: "Fit"
                            iconName: "fit"
                            tip: window.alignmentOpen ? "Show the whole width of the alignment" : window.treeOpen ? "Show the whole tree" : "Show the whole sequence"
                            keys: "Ctrl+0"
                            onInvoked: (window.delimitationOpen ? window.speciesTree : window.current).fit()
                        }
                        RibbonButton { commandId: "ribbon.zoom-to-the-selection"; text: "Selection"; iconName: "selection"; enabled: window.current !== null && (window.currentKind === "sequence" || window.alignmentOpen); tip: "Zoom to the selection"; onInvoked: window.current.zoom_to_selection() }
                        RibbonNumberField {
                            objectName: "zoomPercentage"
                            label: "%"
                            tip: "Zoom percentage"
                            disabledReason: window.delimitationOpen ? "Attach a display tree to zoom it" : "Open a document to change its zoom"
                            readonly property var controller: window.delimitationOpen ? window.speciesTree : window.current
                            from: 0; to: 20000
                            value: controller ? parseFloat(controller.zoom_text) : 100
                            fractional: true
                            enabled: controller !== null
                            onEdited: value => { if (controller) controller.set_zoom_percent(value) }
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true; enabled: window.currentKind === "sequence" }
                    // Linear and circular layouts exist in the code but are
                    // not offered for now; the wrapped layout is the view.
                    RibbonGroup {
                        title: "Show"
                        AppCheckBox {
                            property string keyHint: "V"
                            text: "Overview"
                            checked: window.overviewEnabled
                            enabled: window.current !== null
                            HoverHandler { id: overviewTipHover }
                            Accessible.description: ToolTip.text
                            ToolTip.text: enabled ? "Show a compact navigator when the document extends beyond the view" : "Open a document to show its overview"
                            ToolTip.visible: overviewTipHover.hovered || activeFocus
                            onToggled: window.overviewEnabled = checked
                        }
                        AppCheckBox {
                            property string keyHint: "GC"
                            text: "GC content"
                            enabled: window.currentKind === "sequence"
                            HoverHandler { id: gcTipHover }
                            Accessible.description: ToolTip.text
                            ToolTip.text: enabled ? "Show the GC content plot" : "Open a sequence to show its GC content plot"
                            ToolTip.visible: gcTipHover.hovered || activeFocus
                            ToolTip.delay: 400
                            checked: window.showGc
                            focusPolicy: Qt.StrongFocus
                            onToggled: window.showGc = checked
                        }
                    }
                    Item { Layout.fillWidth: true }
                    RibbonGroup { title: "Find and text"; actions: [findCommand, textRepresentationCommand] }
                    HelpGroup {}
                }

                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        disabledReason: "Open a sequence to use this group"
                        enabled: window.currentKind === "sequence"
                        title: "Annotation"
                        AppRibbonButton { action: addAnnotationCommand; text: "Add"; visible: addAnnotationCommand.visible; iconName: "annotation-add"; tip: addAnnotationCommand.effectiveTooltip }
                        RibbonButton { commandId: "ribbon.edit-the-selected-annotation"; text: "Edit"; iconName: "annotation-edit"; tip: "Edit the selected annotation"; keys: "Enter"; onInvoked: editor.openSelected() }
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
                        disabledReason: "Open a sequence to use this group"
                        enabled: window.currentKind === "sequence"
                        title: "Go to"
                        RibbonButton { commandId: "ribbon.select-the-previous-annotation"; text: "Previous"; iconName: "left"; tip: "Select the previous annotation"; onInvoked: window.current.step_feature(false) }
                        RibbonButton { commandId: "ribbon.select-the-next-annotation"; text: "Next"; iconName: "right"; tip: "Select the next annotation"; onInvoked: window.current.step_feature(true) }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        disabledReason: "Open a sequence to use this group"
                        enabled: window.currentKind === "sequence"
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
                        disabledReason: "Open a sequence to use this group"
                        enabled: window.currentKind === "sequence"
                        title: "Find"
                        RibbonButton { commandId: "sequence.blast"; text: "BLAST"; tip: "Submit sequence queries to NCBI and open browser results"; iconName: "search"; onInvoked: window.beginBlast("whole") }
                        RibbonButton { commandId: "ribbon.annotate-open-reading-frames";
                            text: "ORFs"
                            tip: "Annotate open reading frames"
                            enabled: window.current && !window.current.busy
                            onInvoked: orfDialog.search(false)
                        }
                        RibbonButton { commandId: "ribbon.annotate-protein-coding-genes-with-prodigal";
                            text: "Prodigal"
                            tip: "Annotate protein-coding genes with Prodigal"
                            enabled: window.current && !window.current.busy
                            onInvoked: genesDialog.predict(false)
                        }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }

                // Alignment: edits on the selected block, its columns and rows.
                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
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
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
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
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
                        title: "Rows"
                        RibbonButton { commandId: "ribbon.move-the-selected-rows-up"; text: "Up"; iconName: "up"; tip: "Move the selected rows up"; keys: "Alt+↑"; onInvoked: window.current.move_rows(true) }
                        RibbonButton { commandId: "ribbon.move-the-selected-rows-down"; text: "Down"; iconName: "down"; tip: "Move the selected rows down"; keys: "Alt+↓"; onInvoked: window.current.move_rows(false) }
                        RibbonButton { commandId: "ribbon.rename-the-cursor-s-row"; text: "Rename"; tip: "Rename the cursor's row"; Accessible.name: tip; keys: "F2"; onInvoked: renameDialog.askRow() }
                        RibbonButton { commandId: "ribbon.delete-the-selected-rows"; text: "Delete"; iconName: "delete"; tip: "Delete the selected rows"; onInvoked: window.current.remove_rows() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
                        title: "Find"
                        RibbonButton { commandId: "alignment.blast"; text: "BLAST"; tip: "Submit selected or all FASTA rows to NCBI"; iconName: "search"; onInvoked: window.beginBlast("all") }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
                        title: "Align"
                        RibbonButton { commandId: "ribbon.realign-the-sequences-with-mafft-default-settings-into-a";
                            iconName: "align"
                            text: window.current && window.current.busy ? "Aligning" : "MAFFT"
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
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
                        title: "Add sequence"
                        RibbonButton { commandId: "ribbon.type-a-new-sequence-or-paste-fasta"; text: "Type/paste"; iconName: "new-file"; tip: "Type a new sequence or paste FASTA"; enabled: window.current !== null && !window.current.busy; onInvoked: window.documentAt(tabs.currentIndex).addText() }
                        RibbonButton { commandId: "ribbon.add-all-sequences-from-a-fasta-file"; text: "FASTA"; iconName: "add-file"; tip: "Add all sequences from a FASTA file"; enabled: window.current !== null && !window.current.busy; onInvoked: window.documentAt(tabs.currentIndex).addFasta() }
                        RibbonButton { commandId: "ribbon.review-chromatograms-and-add-their-consensus"; text: "Chromatogram"; iconName: "waveform"; tip: "Review chromatograms and add their consensus"; enabled: window.current !== null && !window.current.busy; onInvoked: window.documentAt(tabs.currentIndex).addTraces() }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }

                // Tree: the selected clade, the order of clades, support
                // values and figures.
                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    TreeRootingGroup {
                        controller: window.treeOpen ? window.current : null
                        treeOpen: window.treeOpen
                        customization: window.customization
                        Component.onCompleted: commands.forEach(command => window.registerRibbonCommand(command))
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        disabledReason: "Open a tree to use this group"
                        enabled: window.treeOpen
                        title: "Clock"
                        RibbonButton {
                            commandId: "tree.rate-smoothing"
                            text: "PL"
                            iconName: "tree-clock"
                            tip: "Smooth rates on the current rooted tree with Sanderson penalized likelihood"
                            enabled: window.treeOpen && window.current !== null && window.currentPath.length > 0
                            onInvoked: operationPicker.showOperation(shell.project_dir.length ? shell.project_dir : shell.local_file_path(shell.parent_folder_url(window.currentPath)), window.currentPath, !!window.current.dirty, "basecraft.sanderson/date_tree@1")
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    TreeCladeGroup {
                        controller: window.treeOpen ? window.current : null
                        customization: window.customization
                        onAnnotateRequested: cladeEditor.openSelected()
                        Component.onCompleted: commands.forEach(command => window.registerRibbonCommand(command))
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    TreeStyleGroup {
                        controller: window.treeOpen ? window.current : null
                        customization: window.customization
                        onExtraRequested: treeStyle.openStyle("extra")
                        onLabelsRequested: metadataTipLabels.show(window.current, window.currentPath)
                        Component.onCompleted: commands.forEach(command => window.registerRibbonCommand(command))
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    // Support values: everything but Add is greyed while the tree has none.
                    RibbonGroup {
                        disabledReason: "Open a tree to use this group"
                        enabled: window.treeOpen
                        id: supportGroup
                        title: "Support"
                        readonly property bool hasSupport: window.treeOpen && window.current !== null && window.current.has_support === true
                        RibbonButton { commandId: "ribbon.add-support-values-from-another-analysis-of-the-same"; text: "Add"; tip: "Add support values from another analysis of the same taxa, matched branch by branch"; onInvoked: supportFile.open() }
                        RibbonNumberField {
                            objectName: "supportFont"
                            label: "Size"
                            tip: "Support value size in pixels"
                            disabledReason: window.treeOpen ? "Use Support > Add to add tree support values before resizing them" : "Open a tree to resize support values"
                            from: 6; to: 30
                            value: window.treeOpen && window.current ? window.current.support_font : 10
                            enabled: supportGroup.hasSupport
                            onEdited: value => window.current.set_sizes(0, value, 0)
                        }
                        RibbonButton { commandId: "ribbon.which-support-values-show-circles-at-nodes-sizes"; text: "Style"; iconName: "support-style"; tip: "Which support values show and how, and circles at nodes"; enabled: supportGroup.hasSupport; onInvoked: treeStyle.openStyle("support") }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    TreeAlignmentGroup {
                        controller: window.treeOpen ? window.current : null
                        customization: window.customization
                        onLinkRequested: (target) => linkAlignmentFile.ask(target)
                        Component.onCompleted: commands.forEach(command => window.registerRibbonCommand(command))
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }

                // Phylogeny: tree inference from the open alignment.
                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
                        title: "Distance tree"
                        Repeater {
                            model: [{label: "Matrix", value: "matrix"}, {label: "NJ", value: "nj"}, {label: "UPGMA", value: "upgma"}]
                            delegate: RibbonButton { commandId: "analysis.distance." + modelData.value;
                                required property var modelData
                                iconName: modelData.value === "matrix" ? "matrix" : modelData.value === "nj" ? "phylogram" : "tree"
                                text: modelData.label
                                enabled: window.current !== null && !window.current.busy
                                tip: modelData.value === "matrix" ? "Save observed distances as TSV and a reusable cache" : modelData.value === "upgma" ? "Build a rooted, equal-rate UPGMA tree" : "Build a neighbor-joining tree"
                                onInvoked: toolSettings.show(window.current, "distance", modelData.value)
                            }
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
                        title: "Maximum likelihood"
                        RibbonButton { commandId: "ribbon.infer-a-tree-with-fasttree-standard-settings-gtr-for";
                            iconName: "tree-fast"
                            text: window.current && window.current.busy ? "Running" : "FastTree"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Infer a tree with FastTree: choose the model, rate variation and support values"
                            onInvoked: toolSettings.show(window.current, "fasttree", "tree")
                        }
                        RibbonButton { commandId: "ribbon.select-a-model-and-infer-a-maximum-likelihood-tree";
                            iconName: "tree-infer"
                            text: "IQ-TREE"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Infer a maximum-likelihood tree: choose the model, rate variation and branch support"
                            onInvoked: toolSettings.show(window.current, "iqtree", "tree")
                        }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }
                RibbonPage {
                    SpeciesMethodsGroup { commandSuffix: "workspace" }
                    RibbonGroup {
                        title: "Specimens"
                        RibbonButton { commandId: "species.roster-links"; text: "Roster"; iconName: "matrix"; enabled: window.speciesView !== null && specimenMetadata.available; tip: "Explicitly link workspace records to project specimens and save partitions"; onInvoked: rosterConsumer.show(window.current, window.currentPath) }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Display"
                        ModeButton { commandId: "species.tree-view"; text: "Tree"; tip: "The tree with a bar column per hypothesis"; iconName: "phylogram"; enabled: window.speciesView !== null && window.current.display_path.length > 0; active: window.speciesView !== null && !window.speciesView.showGroups; onInvoked: window.speciesView.showGroups = false }
                        ModeButton { commandId: "species.text-view"; text: "Text"; tip: "Each hypothesis's species and their sequences, as text"; iconName: "text-representation"; enabled: window.speciesView !== null; active: window.speciesView !== null && window.speciesView.showGroups; onInvoked: window.speciesView.showGroups = true }
                        RibbonButton { commandId: "species.layout"; text: "Layout"; tip: "Bar width, the gap between columns and the space between species, for every column"; iconName: "bar-layout"; enabled: window.speciesView !== null && window.speciesView.columnInfo.columns.length > 0; onInvoked: window.speciesView.layoutBars() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    // The same tree style controls as the tree page, for the
                    // display tree; they change only how it is drawn here.
                    TreeStyleGroup {
                        scope: "species"
                        controller: window.speciesTree
                        customization: window.customization
                        onExtraRequested: treeStyle.openStyle("extra")
                        Component.onCompleted: commands.forEach(command => window.registerRibbonCommand(command))
                        RibbonButton { commandId: "species.support-style"; text: "Support"; iconName: "support-style"; tip: "Which support values show and how, and their size"; enabled: window.speciesTree !== null && window.speciesTree.has_support; onInvoked: treeStyle.openStyle("support") }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Hypotheses"
                        RibbonButton { commandId: "species.new"; text: "New"; tip: "New species hypothesis, empty or copied from another"; iconName: "add"; enabled: window.speciesView !== null && !window.current.busy; onInvoked: window.speciesView.newHypothesis() }
                        RibbonButton { commandId: "species.columns"; text: "Columns"; tip: "Choose which hypotheses get a bar column"; iconName: "columns"; enabled: window.speciesView !== null; onInvoked: window.speciesView.chooseColumns() }
                        ModeButton { commandId: "species.details"; text: "Details"; tip: "Species of the hypothesis being edited, fit report and distances"; iconName: "info"; enabled: window.speciesView !== null; active: window.speciesView !== null && window.speciesView.detailsOpen; onInvoked: window.speciesView.detailsOpen = !window.speciesView.detailsOpen }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Tools"
                        ModeButton { commandId: "species.select-tool"; text: "Select"; tip: "Click a node to select its clade, or a bar to select a species"; iconName: "selection"; enabled: window.speciesView !== null; active: window.speciesView !== null && window.speciesView.tool === "select"; onInvoked: window.speciesView.setTool("select") }
                        ModeButton { commandId: "species.threshold-cut"; text: "Threshold"; tip: window.speciesView !== null && !window.current.can_cut ? "Needs an ultrametric display tree (all tips the same distance from the root)" : "Drag a threshold across the tree and save the species it defines"; iconName: "tree-threshold"; enabled: window.speciesView !== null && !window.current.busy && window.current.can_cut; active: window.speciesView !== null && window.speciesView.tool === "cut"; onInvoked: window.speciesView.setTool("cut") }
                        RibbonButton { commandId: "species.display"; text: "Attach"; tip: window.speciesView !== null && window.current.display_path.length ? "A display tree is attached" : "Attach or build a display tree"; iconName: "tree"; enabled: window.speciesView !== null && !window.current.busy && !window.current.display_path.length; onInvoked: window.beginDelimitation("view") }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }

                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        title: "Calls"
                        disabledReason: !window.traceOpen || !window.current || !window.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
                        enabled: window.traceOpen && window.current && window.current.review_mode && window.traceDocument && !window.traceDocument.working
                        AppComboBox {
                            id: traceCallSets
                            property string keyHint: "T"
                            Accessible.name: "Trace call set"
                            model: window.traceOpen && window.current ? JSON.parse(window.current.call_sets) : []
                            textRole: "label"
                            valueRole: "id"
                            currentIndex: window.traceOpen && window.current ? model.findIndex(row => row.id === window.current.selected_call_set) : -1
                            onActivated: window.current.select_calls(currentValue)
                            HoverHandler { id: callSetTipHover }
                            Accessible.description: ToolTip.text
                            ToolTip.text: !enabled ? (!window.traceOpen || !window.current || !window.current.review_mode ? "Open a Sanger review to choose a call set" : "Wait for trace processing to finish") : "Each call set retains its own reviewed decisions. Computed calls are an alternative, not a guarantee of greater accuracy."
                            ToolTip.visible: callSetTipHover.hovered || activeFocus
                        }
                        RibbonButton { commandId: "trace.compute-calls"; text: "Compute"; tip: "Compute or reuse alternative Tracy calls"; enabled: window.current && !window.current.calling && window.traceDocument && window.traceDocument.traceRows.length === 1; onInvoked: window.current.compute_calls() }
                        RibbonButton { commandId: "trace.cancel-calls"; text: "Cancel"; tip: "Cancel background calling; keep the current review"; enabled: window.current && window.current.calling; onInvoked: window.current.cancel_calls() }
                    }
                    RibbonGroup {
                        title: "Issues"
                        disabledReason: !window.traceOpen || !window.current || !window.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
                        enabled: window.traceOpen && window.traceDocument && !window.traceDocument.working
                        RibbonButton { commandId: "ribbon.previous-unresolved-issue"; iconName: "previous"; tip: "Previous unresolved issue"; keys: "Alt+← or ["; enabled: (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.issue(false) }
                        RibbonButton { commandId: "ribbon.next-unresolved-issue"; iconName: "next"; tip: "Next unresolved issue"; keys: "Alt+→ or ]"; enabled: (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.issue(true) }
                        RibbonButton { commandId: "ribbon.accept-this-base-and-go-to-the-next-issue"; text: "Accept"; iconName: "accept"; tip: "Accept this base and go to the next issue"; keys: "Alt+Enter"; enabled: (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.accept() }
                        RibbonButton { commandId: "ribbon.choose-alignment-context"; text: "Alignment"; tip: "Choose alignment context"; enabled: (window.traceOpen && window.current && window.current.review_mode && window.traceDocument && window.traceDocument.returnAlignment === null); onInvoked: window.traceDocument.alignmentContext() }
                        RibbonButton { commandId: "ribbon.combine-up-to-three-reads"; text: "Draft reads"; tip: "Combine up to three reads"; enabled: (window.traceOpen && window.current && window.traceDocument && window.traceDocument.traceRows.length === 1); onInvoked: window.traceDocument.draftReads() }
                        RibbonButton { commandId: "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft"; text: "Signal cutoff"; tip: "Choose the terminal signal percentage for a new draft"; onInvoked: window.traceDocument.signalSettings() }
                    }
                    RibbonGroup {
                        title: "Masking"
                        disabledReason: !window.traceOpen || !window.current || !window.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
                        enabled: window.traceOpen && window.current && window.current.review_mode && window.traceDocument && !window.traceDocument.working
                        RibbonButton { commandId: "ribbon.choose-the-consensus-range-to-keep-in-exports"; text: "Mask ends"; tip: "Choose the consensus range to keep in exports"; onInvoked: window.traceDocument.maskEnds() }
                        RibbonButton { commandId: "ribbon.find-partial-terminal-primer-candidates-and-mask-them"; text: "Set primers"; tip: "Find partial terminal primer candidates and mask them"; onInvoked: window.traceDocument.setPrimers() }
                        RibbonButton { commandId: "trace.keep-from"; text: "Keep from"; iconName: "trace-mask"; tip: "Keep from the selected consensus column"; onInvoked: window.traceDocument.setMaskStart(window.current.selected_column) }
                        RibbonButton { commandId: "trace.keep-through"; text: "Keep through"; iconName: "trace-mask"; tip: "Keep through the selected consensus column"; onInvoked: window.traceDocument.setMaskEnd(window.current.selected_column) }
                    }
                    RibbonGroup {
                        title: "Export consensus"
                        disabledReason: !window.traceOpen || !window.current || !window.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
                        enabled: window.traceOpen && window.current && window.current.review_mode && window.traceDocument && !window.traceDocument.working
                        ExportConsensusButton { commandId: "ribbon.add-the-edited-consensus-to-its-alignment"; text: "Alignment"; iconName: "consensus-to-alignment"; tip: "Add the edited consensus to its alignment"; keys: "Ctrl+Shift+A"; enabled: window.traceDocument !== null && window.traceDocument.returnAlignment !== null && window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.addToAlignment() }
                        ExportConsensusButton { commandId: "ribbon.export-the-edited-consensus-as-fasta"; text: "Save"; iconName: "fasta-document"; tip: "Export the edited consensus as FASTA"; keys: "Ctrl+Shift+E"; enabled: window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.exportConsensus() }
                        ExportConsensusButton { commandId: "ribbon.copy-the-edited-consensus-as-fasta"; text: "Copy"; iconName: "clipboard"; tip: "Copy the edited consensus as FASTA"; keys: "Ctrl+Shift+C"; enabled: window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.copyConsensus() }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }

                // Export: figures, specimen preparation and the complete GenBank workflow.
                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        title: "Figure"
                        AppRibbonButton { action: exportSvgCommand; visible: exportSvgCommand.visible; text: "SVG"; iconName: "save-as"; tip: exportSvgCommand.effectiveTooltip }
                        AppRibbonButton { action: exportPngCommand; visible: exportPngCommand.visible; text: "PNG"; iconName: "figure-export"; tip: exportPngCommand.effectiveTooltip }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Zenodo"
                        RibbonButton { commandId: "export.zenodo"; text: "Zenodo"; iconName: "save-as"; tip: shell.project_dir.length ? "Prepare selected saved files for a Zenodo draft" : "Choose a project folder first"; enabled: shell.project_dir.length > 0; onInvoked: { if (zenodoDialog.ready || zenodoJobs.busy || zenodoDialog.attempted) zenodoDialog.open(); else zenodoPicker.open() } }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Specimens"
                        RibbonButton { commandId: "metadata.table"; text: "Table"; iconName: "matrix"; tip: shell.project_dir.length ? "The project's specimens, identifications and collection data" : "Choose a project folder first"; enabled: shell.project_dir.length > 0; onInvoked: specimenTable.open() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Import"
                        RibbonButton { commandId: "metadata.import-headers"; text: "Headers"; iconName: "text-representation"; tip: window.metadataFasta.length ? "Read specimen IDs, taxa and values from this file's FASTA headers" : "Open a FASTA file first"; enabled: shell.project_dir.length > 0 && window.metadataFasta.length > 0; onInvoked: window.beginMetadataImport("headers", window.metadataFasta) }
                        RibbonButton { commandId: "metadata.import-sheet"; text: "Spreadsheet"; iconName: "add-file"; tip: "Import specimen data from .ods, .xlsx, CSV or TSV"; enabled: shell.project_dir.length > 0; onInvoked: metadataSheetPicker.open() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Records"
                        RibbonButton { commandId: "metadata.template"; text: "Template"; iconName: "new-file"; tip: window.metadataFasta.length ? "Save an .ods sheet with a row per record of this file, to fill in" : "Open a FASTA file first"; enabled: shell.project_dir.length > 0 && window.metadataFasta.length > 0; onInvoked: metadataTemplateDestination.open() }
                        Label {
                            objectName: "metadataLinkStatus"
                            visible: window.metadataLinks.records !== undefined
                            text: window.metadataLinks.records === undefined ? ""
                                : window.metadataLinks.unlinked.length === 0 ? "All " + window.metadataLinks.records + " records linked"
                                : window.metadataLinks.unlinked.length + " of " + window.metadataLinks.records + " records not linked"
                            Accessible.name: text
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "GenBank"
                        RibbonButton { commandId: "submission.new"; text: "Start"; iconName: "new-file"; tip: window.submissionAlignment.length ? "Prepare a GenBank submission from this alignment" : "Open the marker alignment first"; enabled: shell.project_dir.length > 0 && window.submissionAlignment.length > 0; onInvoked: window.startSubmission() }
                        RibbonButton { commandId: "submission.open"; text: "Open"; iconName: "open"; tip: "Open a GenBank submission of this project"; enabled: shell.project_dir.length > 0; onInvoked: submissionPicker.open() }
                        RibbonButton { commandId: "submission.prepare"; text: "Prepare"; iconName: "edit"; tip: submissions.available ? "Rows, batch settings, reading frames, issues, package and accessions" : "Start or open a submission first"; enabled: submissions.available; onInvoked: window.showSubmission() }
                        RibbonButton { commandId: "submission.validate"; text: "Validate"; iconName: "accept"; tip: "Export the package and check it with table2asn on this computer"; enabled: submissions.available && !submissionJobs.busy; onInvoked: window.validateSubmission() }
                        RibbonButton { commandId: "submission.submitter"; text: "Submitter"; iconName: "project"; tip: "Your contact details for submission templates"; onInvoked: submitterDialog.show(JSON.parse(preferences.snapshot || "{}").submitter) }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }
        }
    }

    // Settings for every plugin run, drawn from the plugin's manifest.
    ToolSettingsDialog { id: toolSettings }
    OperationPickerDialog { id: operationPicker; onOpenResult: (path) => window.openPath(path) }

    ShortcutsDialog { id: shortcuts; commands: window.applicationCommands }

    AppOperations { id: zenodoJobs }
    ZenodoDialog { id: zenodoDialog; backend: zenodoJobs; onChooseFiles: zenodoPicker.open() }
    FileDialog {
        id: zenodoPicker
        title: "Choose saved files for a Zenodo draft"
        fileMode: FileDialog.OpenFiles
        currentFolder: window.activeFolder()
        onAccepted: {
            let defaults = {}
            if (submissions.available) {
                const doc = JSON.parse(submissions.document || "{}")
                defaults = {title: (doc.publication || {}).title || "", creators: doc.publication_authors_text || ""}
            }
            zenodoDialog.show(Array.from(selectedFiles, url => window.localPath(url)), shell.project_dir, defaults)
        }
    }

    // The project's specimen metadata (Export tab). It follows the
    // project folder; opening a project never creates metadata files.
    MetadataController { id: specimenMetadata }
    readonly property string metadataProject: shell.project_dir
    onMetadataProjectChanged: {
        specimenMetadata.use_project(metadataProject)
        submissions.use_project(metadataProject)
    }
    readonly property var metadataTable: JSON.parse(specimenMetadata.table)
    // The open FASTA file whose records can be imported, linked or templated.
    readonly property string metadataFasta: current && (alignmentOpen || currentKind === "sequence")
        && /\.(fa|fasta|fna|fas|ffn|faa|afa)$/i.test(current.file_path || "") ? current.file_path : ""
    // Records of that file without a stored specimen link; only reported.
    readonly property var metadataLinks: metadataFasta.length && specimenMetadata.available && metadataTable
        ? JSON.parse(specimenMetadata.link_status(metadataFasta)) : ({})
    function metadataColumns() {
        const names = metadataTable.columns.map(c => c.name)
        for (const c of metadataTable.addable || []) names.push(c.name)
        return names
    }
    function beginMetadataImport(kind, path) {
        if (!specimenMetadata.available && !specimenMetadata.create()) { specimenTable.open(); return }
        const data = kind === "headers" ? JSON.parse(specimenMetadata.header_preview(path, "|"))
            : JSON.parse(specimenMetadata.sheet_preview(path))
        metadataImport.separator = "|"
        metadataImport.show(kind, path, data, metadataColumns(), metadataTable.patterns)
    }
    SpecimenTableDialog {
        id: specimenTable
        table: window.metadataTable
        canUndo: specimenMetadata.can_undo
        canRedo: specimenMetadata.can_redo
        message: specimenMetadata.message
        messageKind: specimenMetadata.message_kind
        onCreateRequested: specimenMetadata.create()
        onCellEdited: (specimen, column, text) => specimenMetadata.set_cell(specimen, column, text)
        onTaxonEdited: (specimen, name) => specimenMetadata.identify(specimen, name)
        onSpecimenAdded: id => specimenMetadata.add_specimen(id)
        onSpecimenRemoved: id => specimenMetadata.remove_specimen(id)
        onColumnAdded: name => specimenMetadata.add_column(name)
        onUndoRequested: specimenMetadata.undo()
        onRedoRequested: specimenMetadata.redo()
    }
    MetadataImportDialog {
        id: metadataImport
        onPreviewRequested: separator => metadataImport.setPreview(JSON.parse(specimenMetadata.header_preview(metadataImport.file, separator)))
        onPlanRequested: (separator, roles, createSpecimens, overwrite) => {
            const ok = metadataImport.mode === "headers"
                ? specimenMetadata.plan_headers(metadataImport.file, separator, roles, createSpecimens, overwrite)
                : specimenMetadata.plan_sheet(metadataImport.file, roles, metadataImport.preview.has_document ? "" : window.metadataFasta, createSpecimens, overwrite)
            if (!ok) { metadataImport.message = specimenMetadata.message; return }
            metadataImport.close()
            metadataReview.show(JSON.parse(specimenMetadata.plan), metadataImport.mode === "headers" ? "pattern" : "mapping")
        }
    }
    // GenBank marker submissions (Export tab). table2asn runs as a
    // registered operation on its own job runner, like BLAST.
    SubmissionController { id: submissions }
    Connections {
        target: specimenMetadata
        function onTableChanged() { if (submissions.available) submissions.refresh_review() }
    }
    // The open alignment a submission can start from.
    readonly property string submissionAlignment: alignmentOpen ? metadataFasta : ""
    AppOperations { id: submissionJobs }
    Timer { interval: 200; repeat: true; running: submissionJobs.busy; onTriggered: {
        submissionJobs.poll()
        if (!submissionJobs.busy) submissions.record_validation(submissionJobs.outcome)
    } }
    function startSubmission() {
        const file = submissionAlignment.split("/").pop()
        submissionName.ask("Start a GenBank submission", "Submission name", file.replace(/\.[^.]*$/, ""), name => {
            if (!submissions.create(submissionAlignment, name)) return submissions.message
            showSubmission()
            return ""
        })
    }
    function openSubmission(path) { if (submissions.open(path)) showSubmission() }
    function showSubmission() {
        submissions.refresh_review()
        submissionDialog.open()
    }
    function validateSubmission() {
        if (!submissions.export_package()) { showSubmission(); return }
        const request = submissions.validation_request()
        if (request.length) submissionJobs.submit(shell.project_dir, request)
        if (!submissionDialog.opened) showSubmission()
    }
    TextPromptDialog { id: submissionName }
    SubmissionDialog {
        id: submissionDialog
        document: JSON.parse(submissions.document || "null")
        review: JSON.parse(submissions.review || '{"rows":[],"issues":[]}')
        pkg: JSON.parse(submissions.package || "null")
        geneticCodes: JSON.parse(submissions.genetic_codes())
        path: submissions.path
        message: submissionJobs.busy ? submissionJobs.message : submissions.message
        messageKind: submissions.message_kind
        canUndo: submissions.can_undo
        canRedo: submissions.can_redo
        validating: submissionJobs.busy
        reviewStage: submissions.review_stage
        validationStage: submissionJobs.message
        reviewing: submissions.reviewing
        parseAuthors: text => JSON.parse(submissions.preview_authors(text))
        onDetailsRequested: details => submissions.set_details(details)
        onInclusionPreviewRequested: rule => { inclusionPlan = JSON.parse(submissions.preview_inclusion(rule)) }
        onInclusionPresetsRequested: save => {
            const result = JSON.parse(specimenMetadata.inclusion_presets(save))
            presetError = result.error || ""
            if (!result.error) inclusionPresets = result
        }
        onEditRequested: edit => submissions.edit(edit)
        onPublicationRequested: (title, authors) => submissions.set_publication(title, authors)
        onDecisionRequested: (record, decision, accession) => submissions.set_decision(record, decision, accession)
        onFrameRequested: (record, codonStart) => submissions.set_frame(record, codonStart)
        onConfirmDetectedRequested: submissions.confirm_detected()
        onUndoRequested: submissions.undo()
        onRedoRequested: submissions.redo()
        onExportRequested: submissions.export_package()
        onValidateRequested: window.validateSubmission()
        onCancelValidationRequested: submissionJobs.cancel()
        onAccessionsMatchRequested: text => {
            const plan = submissions.plan_accessions(text)
            submissionDialog.accessionPlan = plan.length ? JSON.parse(plan) : null
        }
        onAccessionsRecordRequested: accessions => submissions.record_accessions(accessions)
        onAccessionTableRequested: accessionTableDestination.open()
    }
    SubmitterDialog {
        id: submitterDialog
        onSaved: value => {
            preferences.update("submitter", JSON.stringify(value))
            if (submissions.available) submissions.refresh_review()
        }
    }
    FileDialog {
        id: submissionPicker
        title: "Open a GenBank submission"
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Submissions (*.submission.json)"]
        onAccepted: window.openSubmission(window.localPath(selectedFile))
    }
    FileDialog {
        id: accessionTableDestination
        title: "Save the accession table"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "tsv"
        nameFilters: ["Tab-separated table (*.tsv)"]
        onAccepted: submissions.write_accession_table(window.localPath(selectedFile))
    }
    MetadataReviewDialog {
        id: metadataReview
        onApplyRequested: remember => specimenMetadata.apply_plan(remember)
        onDropped: specimenMetadata.clear_plan()
    }
    FileDialog {
        id: metadataSheetPicker
        title: "Import specimen data from a spreadsheet"
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Spreadsheets (*.ods *.xlsx *.xls *.csv *.tsv)", "All files (*)"]
        onAccepted: window.beginMetadataImport("sheet", window.localPath(selectedFile))
    }
    FileDialog {
        id: metadataTemplateDestination
        title: "Save the specimen data template"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "ods"
        nameFilters: ["OpenDocument spreadsheet (*.ods)"]
        onAccepted: {
            if (!specimenMetadata.available) specimenMetadata.create()
            specimenMetadata.write_template(window.metadataFasta, window.localPath(selectedFile))
        }
    }

    AppContentDialog {
        id: orfDialog
        initialFocusItem: codons
        property bool inSelection: false
        property var source: null
        property string sourcePath: ""
        function search(selectionOnly) {
            if (!window.current || window.current.busy) return
            source = window.current
            sourcePath = source.file_path
            inSelection = selectionOnly
            open()
        }
        title: inSelection ? "Find open reading frames in the selection" : "Find open reading frames"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: codons.forceActiveFocus()
        onAccepted: {
            window.orfMinCodons = codons.value
            if (source && source.file_path === sourcePath && !source.busy) source.find_orfs(codons.value, orfDialog.inSelection)
        }
        body: ColumnLayout {
            spacing: 8
            RowLayout {
                Label { id: codonsLabel; text: "At least" }
                AppSpinBox {
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
        property var source: null
        property string sourcePath: ""
        function predict(selectionOnly) {
            if (!window.current || window.current.busy) return
            source = window.current
            sourcePath = source.file_path
            inSelection = selectionOnly
            const code = source.genetic_code()
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
        onAccepted: if (source && source.file_path === sourcePath && !source.busy) source.predict_genes(geneticCodes.get(codeBox.currentIndex).code, inSelection)
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

    AboutDialog {
        id: about
        version: shell.version()
        backgroundColor: shell.background
        onShortcutsRequested: shortcutsCommand.trigger()
    }

    FileDialog {
        id: openDialog
        title: "Open a sequence, alignment, trace, review or tree"
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Sequences, alignments, traces and trees (*.gb *.gbk *.genbank *.gbff *.fasta *.fa *.fna *.fas *.ffn *.faa *.afa *.ab1 *.abi *.json *.nwk *.newick *.tre *.tree *.trees *.treefile *.contree *.nex *.nexus *.nxs)", "All files (*)"]
        onAccepted: window.openPath(window.localPath(selectedFile))
    }

    property var pendingEvidenceSave: null
    function saveDocument(index, path, continuation) {
        saveTarget(documentTarget(index), path, continuation)
    }
    function saveTarget(owner, path, continuation) {
        const index = targetIndex(owner)
        if (index < 0) { if (continuation) continuation(false); return }
        if (!path && owner.path.startsWith("untitled:")) {
            saveDialog.target = owner
            saveDialog.continuation = continuation
            saveDialog.selectedFile = shell.local_file_url(owner.path.split("/").pop())
            saveDialog.open()
            return
        }
        const target = path || owner.path
        const conflict = JSON.parse(provenance.save_conflicts(target))
        if (conflict.error) { provenance.notification(target, conflict.error, "fail"); return }
        const perform = function() {
            const row = targetIndex(owner)
            if (row < 0) { if (continuation) continuation(false); return }
            let success
            if (path) {
                success = owner.controller.save_as(path)
                if (success) {
                    const savedRow = targetIndex(owner)
                    if (savedRow < 0) { if (continuation) continuation(false); return }
                    openFiles.setProperty(savedRow, "path", path)
                    documentOpens.path_changed(owner.token, path, owner.controller.file_path)
                    owner.path = path
                    Qt.callLater(window.refreshCurrent)
                }
            } else { owner.controller.save(); success = !owner.controller.dirty }
            if (continuation) continuation(success)
        }
        if ((conflict.analyses || []).length) {
            pendingEvidenceSave = {owner: owner, target: target, perform: perform}
            evidenceSaveDialog.conflict = conflict
            evidenceSaveDialog.errorText = ""
            evidenceSaveDialog.open()
        } else perform()
    }
    function validEvidenceSave() {
        const pending = pendingEvidenceSave
        if (pending && targetIndex(pending.owner) >= 0) return pending
        pendingEvidenceSave = null
        evidenceSaveDialog.close()
        return null
    }
    AnalysisEvidenceDialog {
        id: evidenceSaveDialog
        objectName: "evidenceSaveDialog"
        onRejected: window.pendingEvidenceSave = null
        onRelinkOriginal: function(path) {
            const pending = window.validEvidenceSave()
            if (!pending) return
            for (const analysis of conflict.analyses || []) {
                if (analysis.direct === false) continue
                if (!provenance.relink_run(analysis.run, path)) { errorText = provenance.message; return }
            }
            conflict = JSON.parse(provenance.save_conflicts(pending.target))
            errorText = "Original input verified. Cancel to review it, or choose how to continue saving."
            if (!(conflict.analyses || []).length) {
                window.pendingEvidenceSave = null
                close()
                pending.perform()
            }
        }
        onChoice: function(policy) {
            const pending = window.validEvidenceSave()
            if (!pending) return
            if (!provenance.resolve_save(pending.target, policy)) {
                errorText = provenance.message
                return
            }
            window.pendingEvidenceSave = null
            close()
            pending.perform()
        }
    }
    function saveBeforeQuit(targets, offset) {
        if (offset >= targets.length) {
            const left = window.unsavedTabs()
            if (left.length) {
                quitDialog.unsaved = left
                quitError.text = "Some files could not be saved. Review their notifications and retry."
                quitDialog.open()
            } else { window.closeConfirmed = true; window.close() }
            return
        }
        if (targetIndex(targets[offset]) < 0) {
            quitDialog.ask(window.unsavedTabs())
            quitError.text = "The open documents changed. Review them before closing."
            return
        }
        window.saveTarget(targets[offset], "", function(success) {
            if (success) window.saveBeforeQuit(targets, offset + 1)
        })
    }

    // Save the active tab under a new name; the tab then shows the new file.
    function saveAs() {
        if (!window.current) return
        if (window.traceOpen) { window.traceDocument.saveReview(); return }
        saveDialog.target = documentTarget(tabs.currentIndex)
        saveDialog.continuation = null
        saveDialog.selectedFile = shell.local_file_url(window.currentPath.startsWith("untitled:") ? window.fileName(window.currentPath) : window.currentPath)
        saveDialog.open()
    }

    Shortcut { sequence: "Ctrl+Q"; onActivated: window.close() }
    // Close the active tab, asking first if it has unsaved changes.
    Shortcut { sequence: "Ctrl+W"; onActivated: if (openFiles.count > 0) window.closeTab(tabs.currentIndex) }
    // Undo and redo reach the open document wherever the focus is; a text
    // field being edited keeps its own. Trace review has its own undo.
    Shortcut { sequence: "Ctrl+Z"; enabled: !submissionDialog.opened && window.current !== null && !window.traceOpen; onActivated: window.current.undo() }
    Shortcut { sequences: ["Ctrl+Shift+Z", "Ctrl+Y"]; enabled: !submissionDialog.opened && window.current !== null && !window.traceOpen; onActivated: window.current.redo() }
    FileDialog {
        id: saveDialog
        objectName: "saveDocumentDialog"
        onRejected: { target = null; continuation = null }
        property var target: null
        property var continuation: null
        title: "Save as"
        fileMode: FileDialog.SaveFile
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        onAccepted: {
            const owner = target
            const path = window.localPath(selectedFile)
            const next = continuation
            target = null
            continuation = null
            window.saveTarget(owner, path, next)
        }
    }

    Component {
        id: projectDialogComponent
        FolderDialog {
            parentWindow: window
            title: "Open or create a project folder"
            onAccepted: {
                window.selectProject(window.localPath(selectedFolder))
                window.projectDialog = null
                destroy()
            }
            onRejected: { window.projectDialog = null; destroy() }
        }
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
                onCurrentIndexChanged: { if (!window.applyingOpenFocus) documentOpens.revoke_focus(); Qt.callLater(window.refreshCurrent) }
                Repeater {
                    model: openFiles
                    AppTabButton {
                        required property int index
                        required property string path
                        required property bool dirty
                        onClicked: { documentOpens.revoke_focus(); window.focusOpenedDocument = true; Qt.callLater(window.refreshCurrent) }
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

            // Activity belongs to this document, beside its tabs and canvas.
            AppMessageBar {
                objectName: "documentActivity"
                Layout.fillWidth: true
                readonly property var controller: window.current
                busy: !!controller && (window.currentKind === "sequence" || window.alignmentOpen || window.treeOpen) && controller.busy
                visible: busy
                workingText: busy ? (window.alignmentOpen && controller.bootstrap_running
                    ? controller.bootstrap_status : controller.message) || "Working" : ""
                cancellable: busy && window.alignmentOpen && controller.bootstrap_running && !controller.bootstrap_cancelling
                cancelText: "Cancel bootstrap"
                onCancelRequested: controller.cancel_bootstrap()
            }

            StackLayout {
                id: stack
                visible: openFiles.count > 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: tabs.currentIndex

                Repeater {
                    id: docs
                    model: openFiles
                    onItemAdded: Qt.callLater(window.refreshCurrent)
                    onItemRemoved: (index, item) => {
                        window.retireDelimitationDocument(item.adoptedSlot === true ? item.documentView : item)
                        Qt.callLater(window.refreshCurrent)
                    }
                    // Each file opens in the editor for what it holds.
                    delegate: DelegateChooser {
                        role: "kind"
                        DelegateChoice {
                            roleValue: "canvas"
                            Item {
                                id: canvasSlot
                                required property string path
                                required property string openToken
                                required property int index
                                readonly property bool adoptedSlot: true
                                property bool ready: false
                                readonly property var documentView: canvasLoader.item
                                property var admittedController: null
                                property bool alive: true
                                Component.onDestruction: {
                                    alive = false
                                    documentOpens.retire(openToken)
                                    if (admittedController) admittedController.destroy()
                                }
                                Component { id: sequenceOwner; GenomeController {} }
                                Component { id: alignmentOwner; AlignmentController {} }
                                Component { id: treeOwner; TreeController {} }
                                function construct(kind) {
                                    if (!alive || admittedController) return
                                    const factory = kind === "sequence" ? sequenceOwner : kind === "alignment" ? alignmentOwner : treeOwner
                                    const owner = factory.createObject(canvasSlot)
                                    if (!owner) { constructionFailed(); return }
                                    admittedController = owner
                                    if (!documentOpens.install(openToken, owner)) { constructionFailed(); return }
                                    const values = {path: path, index: index, suppliedController: owner,
                                        overviewEnabled: window.overviewEnabled, notificationSink: provenance}
                                    if (kind === "sequence") { values.showGc = window.showGc; values.defaultLayout = window.defaultLayout }
                                    if (kind === "alignment") { values.fileUrls = shell; values.insertMode = window.alignmentInsert }
                                    canvasLoader.setSource(kind === "sequence" ? "DocumentView.qml" : kind === "alignment" ? "AlignmentView.qml" : "PhyloView.qml", values)
                                }
                                function constructionFailed() {
                                    if (!alive) return
                                    const token = openToken
                                    Qt.callLater(() => {
                                        const row = window.requestIndex(token)
                                        if (row >= 0) {
                                            provenance.notification(openFiles.get(row).path, "Could not construct the document view", "fail")
                                            window.removeDocument(row)
                                        }
                                    })
                                }
                                Loader {
                                    id: canvasLoader
                                    anchors.fill: parent
                                    onStatusChanged: if (status === Loader.Error) canvasSlot.constructionFailed()
                                }
                                Binding { target: canvasLoader.item; property: "overviewEnabled"; value: window.overviewEnabled; when: canvasLoader.item !== null }
                                Binding { target: canvasLoader.item; property: "showGc"; value: window.showGc; when: canvasLoader.item !== null && canvasLoader.item.kind === "sequence" }
                                Binding { target: canvasLoader.item; property: "insertMode"; value: window.alignmentInsert; when: canvasLoader.item !== null && canvasLoader.item.kind === "alignment" }
                                Binding { target: canvasLoader.item; property: "index"; value: canvasSlot.index; when: canvasLoader.item !== null }
                                Binding { target: canvasLoader.item; property: "path"; value: canvasSlot.path; when: canvasLoader.item !== null }
                                Connections {
                                    target: canvasLoader.item
                                    ignoreUnknownSignals: true
                                    function onInitializationCompleted() {
                                        if (!canvasSlot.alive || !documentOpens.acknowledge(canvasSlot.openToken, canvasSlot.admittedController)) return
                                        canvasSlot.ready = true
                                        documentOpens.activate(canvasSlot.openToken)
                                        window.refreshCurrent(false)
                                    }
                                    function onDirtyChanged() { openFiles.setProperty(canvasSlot.index, "dirty", canvasLoader.item.dirty) }
                                    function onActivate() { tabs.currentIndex = canvasSlot.index }
                                    function onOpenFile(path) { window.openPath(path) }
                                    function onReviewTraces(paths, reference) { window.startTraceReview(paths, reference, canvasLoader.item) }
                                    function onToggleInsert() { window.alignmentInsert = !window.alignmentInsert }
                                    function onContextMenu(info) {
                                        if (canvasLoader.item.kind === "tree") treeMenu.showFor(canvasSlot.admittedController, info)
                                        else if (canvasLoader.item.kind === "alignment") { alnMenu.info = info; alnMenu.popup() }
                                        else { menu.info = info; menu.popup() }
                                    }
                                    function onCopyText(text) { clipboard.put(text) }
                                    function onToggleSidebar() { window.sidebarOpen = !window.sidebarOpen }
                                    function onRenameRow() { renameDialog.askRow() }
                                    function onRenameTip() { tipDialog.ask() }
                                    function onAnnotate() { cladeEditor.openSelected() }
                                    function onEditFeature(id) { editor.openFeature(id) }
                                    function onEditSelected() { editor.openSelected() }
                                }
                                AppMessageBar {
                                    anchors { left: parent.left; right: parent.right; top: parent.top }
                                    visible: !canvasSlot.ready
                                    busy: true
                                    workingText: "Opening " + window.fileName(canvasSlot.path)
                                }
                            }
                        }
                        DelegateChoice {
                            roleValue: "trace"
                            SangerView {
                                overviewEnabled: window.overviewEnabled
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
                                overviewEnabled: window.overviewEnabled
                                notificationSink: provenance
                                fileUrls: shell
                                insertMode: window.alignmentInsert
                                onActivate: tabs.currentIndex = index
                                onReviewTraces: (paths, reference) => {
                                    window.startTraceReview(paths, reference, window.documentAt(tabs.currentIndex))
                                }
                                onToggleInsert: window.alignmentInsert = !window.alignmentInsert
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { alnMenu.info = info; alnMenu.popup() }
                                onCopyText: (text) => clipboard.put(text)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameRow: renameDialog.askRow()
                            }
                        }
                        DelegateChoice {
                            roleValue: "tree"
                            // Not "TreeView": that's Qt's, used by the
                            // files sidebar.
                            PhyloView {
                                overviewEnabled: window.overviewEnabled
                                id: treeDocument
                                notificationSink: provenance
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => treeMenu.showFor(treeDocument.controller, info)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameTip: tipDialog.ask()
                                onAnnotate: cladeEditor.openSelected()
                            }
                        }
                        DelegateChoice {
                            roleValue: "delimitation"
                            DelimitationView {
                                overviewEnabled: window.overviewEnabled
                                notificationSink: provenance
                                onOpenFile: (path) => window.openPath(path)
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                            }
                        }
                        DelegateChoice {
                            DocumentView {
                                id: sequenceDocument
                                overviewEnabled: window.overviewEnabled
                                notificationSink: provenance
                                showGc: window.showGc
                                defaultLayout: window.defaultLayout
                                onDirtyChanged: openFiles.setProperty(index, "dirty", dirty)
                                onContextMenu: (info) => { menu.info = info; menu.popup() }
                                onEditFeature: (id) => editor.openFeature(id)
                                onEditSelected: editor.openSelected()
                                onInitializeNew: window.initializeSequence(sequenceDocument)
                                onCopyText: (text) => clipboard.put(text)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                            }
                        }
                    }
                }
            }

            EmptyWorkspaceBrand {
                objectName: "emptyWorkspaceBrand"
                visible: openFiles.count === 0 && shell.project_dir.length === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                backgroundColor: shell.background
                recentProjects: window.recentProjects
                onOpenProject: window.openProjectPicker()
                onOpenRecent: path => window.selectProject(path)
            }

            Item {
                visible: openFiles.count === 0 && shell.project_dir.length > 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                EmptyView {
                    objectName: "emptyProjectDocuments"
                    anchors.centerIn: parent
                    width: Math.min(parent.width, 48 * font.pixelSize)
                    message: "Open a sequence, alignment, chromatogram trace, tree, Sanger review, species delimitation workspace, specimen metadata table or GenBank submission. Choose a file on the left, or use Open to browse another folder."
                    action: openCommand
                    actionText: "Open"
                }
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
                    // A tree's linked alignment, such as "No linked alignment".
                    Label {
                        objectName: "treeAlignmentStatus"
                        readonly property string linkStatus: window.treeOpen && window.current ? (window.current.alignment_link_status || "") : ""
                        visible: linkStatus.length > 0
                        text: linkStatus
                        Layout.maximumWidth: 360
                        elide: Text.ElideMiddle
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

    AppOperations { id: blastBackend }
    BlastDialog { id: blastDialog; backend: blastBackend; onCopyText: text => clipboard.put(text) }
    function beginBlast(mode, feature) {
        const controller = window.current
        if (!window.alignmentOpen && mode === "whole" && controller.has_selection()) mode = "selection"
        const annotation = window.alignmentOpen ? -1 : (feature === undefined ? controller.selected_feature() : feature)
        blastDialog.show(source => controller.blast_queries(source, annotation), mode, window.alignmentOpen)
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
        Item { text: "BLAST annotation at NCBI"; visible: menu.onAnnotation; onTriggered: window.beginBlast("annotation", menu.info.feature) }
        Item { text: "Extract as new sequence"; visible: menu.onAnnotation; onTriggered: window.current.extract_feature(menu.info.feature) }
        Item { text: "Zoom to annotation"; visible: menu.onAnnotation; onTriggered: window.current.zoom_to_feature(menu.info.feature) }

        Item { text: "Add annotation…"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: addAnnotationCommand.trigger() }
        Item {
            text: "Copy sequence (" + menu.info.length + " bp)"
            visible: !menu.onAnnotation
            enabled: menu.info.selection
            onTriggered: clipboard.put(window.current.selection_text())
        }
        Item { text: "Extract as new sequence"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: window.current.extract_selection() }
        Item { text: "Zoom to selection"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: window.current.zoom_to_selection() }
        Item { text: "BLAST selection at NCBI"; visible: !menu.onAnnotation; enabled: menu.info.selection; onTriggered: window.beginBlast("selection") }
        Item { text: "Find ORFs in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection && window.current && !window.current.busy; onTriggered: orfDialog.search(true) }
        Item { text: "Predict genes in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection && window.current && !window.current.busy; onTriggered: genesDialog.predict(true) }
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
        AlnItem { text: "Rename “" + alnMenu.info.rowName + "”…"; visible: alnMenu.info.rows === 1 && !alnMenu.columns; onTriggered: renameDialog.askRow() }
        AlnItem { text: "Move to top"; visible: alnMenu.rows; onTriggered: window.current.rows_to_top() }
        AlnItem { text: alnMenu.info.rows === 1 ? "Delete row" : "Delete " + alnMenu.info.rows + " rows"; visible: alnMenu.rows; onTriggered: window.current.remove_rows() }
        AlnItem { text: "Insert gap column"; visible: alnMenu.columns; onTriggered: window.current.insert_column() }
        AlnItem { text: alnMenu.info.columns === 1 ? "Delete column" : "Delete " + alnMenu.info.columns + " columns"; visible: alnMenu.columns; onTriggered: window.current.delete_columns() }
        AlnItem { text: "Strip gap-only columns"; visible: alnMenu.columns || alnMenu.rows; onTriggered: window.current.remove_gap_columns() }
        AlnItem { text: "BLAST selected rows at NCBI"; onTriggered: window.beginBlast("rows") }
        AlnItem { text: "Copy as FASTA"; onTriggered: clipboard.put(window.current.selection_text()) }
    }

    TextPromptDialog {
        id: renameDialog
        function askRow() {
            if (window.alignmentOpen)
                ask("Rename row", "Row name", window.current.row_name(), value => window.current.rename_row(value))
        }
    }

    // Tree context menu: what applies to the clade or tip under the pointer.
    TreeContextMenu {
        id: treeMenu
        onNameRequested: (target, annotation, revision) => cladeEditor.openCaptured(target, annotation, revision, true)
        onRenameTipRequested: (target, name, revision) => tipDialog.openCaptured(target, name, revision)
        onRootPositionRequested: rootDialog.ask()
        onSupportRequested: supportFile.open()
        onStyleRequested: treeStyle.openStyle("extra")
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

    SpecimenTreeLabelsDialog {
        id: metadataTipLabels
        metadataController: specimenMetadata
        table: window.metadataTable
        projectDir: shell.project_dir
    }
    SpecimenRosterDialog {
        id: rosterConsumer
        metadataController: specimenMetadata
        table: window.metadataTable
        projectDir: shell.project_dir
    }
    TreeTipNameDialog {
        id: tipDialog
        controller: window.treeOpen ? window.current : null
    }

    // Support from another tree: pick the file, then name the values.
    FileDialog {
        id: supportFile
        title: "Add support values from another tree"
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Trees (*.nwk *.newick *.tre *.tree *.trees *.treefile *.contree *.nex *.nexus *.nxs)", "All files (*)"]
        onAccepted: {
            supportName.controller = window.current
            supportName.path = window.localPath(selectedFile)
            supportName.metadata = JSON.parse(supportName.controller.support_import_json(supportName.path))
            supportName.open()
        }
    }
    AppContentDialog {
        id: supportName
        initialFocusItem: supportNameField
        property string path
        property var controller: null
        property var metadata: ({sets: []})
        title: "Name the support values"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: { supportNameField.text = metadata.sets.length === 1 ? metadata.sets[0].name : ""; supportNameField.forceActiveFocus() }
        onAccepted: if (controller && !metadata.error) controller.add_support_from(path, supportNameField.text)
        body: ColumnLayout {
            spacing: 6
            Label { text: window.fileName(supportName.path) }
            Label { text: supportName.metadata.error || supportName.metadata.sets.map(set => set.name).join(" / "); wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label { id: supportNameFieldLabel; text: supportName.metadata.sets.length > 1 ? "Optional prefix (individual names are retained)" : "Support values name" }
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
        controller: window.treeOpen ? window.current : window.speciesTree
    }

    FileDialog {
        id: linkAlignmentFile
        property var editController: null
        property string editRevision: ""
        function ask(target) {
            editController = target
            editRevision = target.record_revision()
            open()
        }
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        title: "Link an alignment to this tree"
        nameFilters: ["FASTA alignments (*.fasta *.fa *.fas *.afa)"]
        onAccepted: {
            if (!editController) return
            if (editController.record_revision() !== editRevision) {
                editController.message_kind = "error"
                editController.message = "The tree changed. Choose the linked alignment again."
                return
            }
            editController.link_alignment(window.localPath(selectedFile))
        }
    }

    function exportSvg(format) {
        svgOptions.format = format || "svg"
        if (!window.current) return
        svgDialog.controller = window.current
        svgDialog.sourcePath = window.currentPath
        svgScope.currentIndex = window.treeOpen ? 2 : 0
        svgPublication.checked = false
        svgSelection.checked = false
        svgOptions.alignment = window.alignmentOpen
        svgOptions.open()
    }
    AppContentDialog {
        id: svgOptions
        initialFocusItem: svgScope
        property string format: "svg"
        property bool alignment: false
        title: format === "png" ? "Export PNG" : "Export editable SVG"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        body: ColumnLayout {
            Label { id: svgScopeLabel; text: "Region" }
            AppComboBox { id: svgScope; objectName: "svgScope"; Accessible.labelledBy: svgScopeLabel; Accessible.name: svgScopeLabel.text; model: ["Visible view", "Selection", "Complete figure"] }
            AppCheckBox { id: svgPublication; text: "Publication colours (white background)" }
            AppCheckBox { id: svgSelection; objectName: "svgSelection"; text: "Include selection highlighting"; visible: svgOptions.alignment }
            RowLayout {
                visible: svgOptions.format === "png"
                Label { id: pngWidthLabel; text: "PNG width (pixels)"; visible: svgOptions.format === "png" }
                AppSpinBox {
                    id: pngWidth
                    objectName: "pngWidth"
                    visible: svgOptions.format === "png"
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
            svgDialog.format = svgOptions.format
            svgDialog.scope = svgScope.currentIndex
            svgDialog.publication = svgPublication.checked
            svgDialog.alignment = svgOptions.alignment
            svgDialog.includeSelection = svgSelection.checked
            svgDialog.rasterWidth = pngWidth.value
            svgDialog.selectedFile = shell.local_file_url(svgDialog.sourcePath.replace(/\.[^./]*$/, "") + "." + svgDialog.format)
            svgDialog.open()
        }
    }
    FileDialog {
        id: svgDialog
        currentFolder: window.activeFolder()
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
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
            if (alignment && format === "png") controller.export_figure_png_selection(window.localPath(selectedFile), scope, publication, rasterWidth, includeSelection)
            else if (alignment) controller.export_figure_selection(window.localPath(selectedFile), scope, publication, includeSelection)
            else if (format === "png") controller.export_figure_png(window.localPath(selectedFile), scope, publication, rasterWidth)
            else controller.export_figure(window.localPath(selectedFile), scope, publication)
        }
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
                    onClicked: { draftQuitDialog.close(); window.openPath(modelData, true) }
                }
            }
        }
        onDiscarded: { window.discardAuthoredOnQuit = true; window.close() }
    }

    // Quitting with unsaved changes: save them all, throw them away, or
    // stay.
    AppFormDialog {
        id: quitDialog
        objectName: "quitDocumentsDialog"
        onRejected: window.discardAuthoredOnQuit = false
        property var unsaved: []
        title: "Save changes before closing?"
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
        function ask(targets) {
            unsaved = targets
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
                    required property var modelData
                    text: "  " + window.fileName(modelData.path)
                    font.bold: true
                }
            }
            AppErrorLabel { id: quitError; visible: text.length > 0; color: shell.accent; wrapMode: Text.Wrap }
        }
        onAccepted: {
            window.saveBeforeQuit(unsaved, 0)
        }
        onDiscarded: {
            const current = window.unsavedTabs()
            if (unsaved.some(target => window.targetIndex(target) < 0) ||
                    current.some(target => !unsaved.some(consent => consent.token === target.token && consent.controller === target.controller && consent.path === target.path))) {
                ask(current)
                quitError.text = "The open documents changed. Review them before closing."
                return
            }
            close()
            window.closeConfirmed = true
            window.close()
        }
    }

    AppFormDialog {
        id: confirmClose
        objectName: "closeDocumentDialog"
        property var target: null
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
            const owner = target
            if (window.targetIndex(owner) < 0) return
            window.saveTarget(owner, "", function(success) {
                const row = window.targetIndex(owner)
                if (row < 0) return
                if (!success) {
                    confirmClose.target = owner
                    confirmClose.saveError = owner.controller.message || "Save the review to a file first"
                    confirmClose.open()
                    return
                }
                confirmClose.saveError = ""
                window.removeDocument(row)
                Qt.callLater(window.refreshCurrent)
            })
        }
        onDiscarded: {
            const row = window.targetIndex(target)
            close()
            if (row < 0) return
            window.removeDocument(row)
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
                const d = window.documentAt(i)
                if (d) {
                    d.controller.check_theme()
                    // The species view draws its tree with its own controller.
                    if (d.kind === "delimitation") d.treeController.check_theme()
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
            if (step.action === "start") window.documentAt(tabs.currentIndex).startAnalysis(step.method, step.input, step.rooted || false, JSON.stringify(step.settings || {}), step.display || "existing")
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
    // Exercise the actual project command and native picker for integration checks.
    Timer {
        id: projectPickerCheck
        interval: 150
        repeat: true
        property var steps: []
        property string previousAction: ""
        onTriggered: {
            if (previousAction.length) console.log("PROJECT_PICKER_AUDIT", JSON.stringify({
                action: previousAction, folder: projectDialog ? projectDialog.currentFolder.toString() : window.projectPickerFolder().toString(),
                policyFolder: window.projectPickerFolder().toString(),
                visible: projectDialog !== null && projectDialog.visible, project: shell.project_dir,
                historyError: window.recentProjectsError, preferencesError: preferences.error,
                preferencesReady: window.preferencesReady
            }))
            if (!steps.length) { stop(); Qt.quit(); return }
            const step = steps.shift()
            previousAction = step.action
            if (step.action === "open") projectCommand.trigger()
            else if (step.action === "browse") projectDialog.selectedFolder = shell.local_file_url(step.path)
            else if (step.action === "choose") projectDialog.selectedFolder = shell.local_file_url(step.path)
            else if (step.action === "accept") projectDialog.accept()
            else if (step.action === "reject") projectDialog.reject()
            else if (step.action === "select") window.selectProject(step.path)
        }
    }
    Timer {
        interval: 900
        running: arg("--screenshot") !== undefined
        onTriggered: {
            if (arg("--project-picker")) {
                projectPickerCheck.steps = JSON.parse(arg("--project-picker"))
                projectPickerCheck.start()
                return
            }
            const c = window.current
            const v = window.currentView
            if (!c) { console.log("no document open"); shot.start(); return }
            if (arg("--trace-review")) {
                window.startTraceReview(JSON.parse(arg("--trace-review")), c.review_snapshot(), window.documentAt(tabs.currentIndex))
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
                const trace = window.documentAt(tabs.currentIndex)
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
            if (arg("--add-text-dialog")) window.documentAt(tabs.currentIndex).addText()
            if (arg("--add-fasta-dialog")) window.documentAt(tabs.currentIndex).prepareFasta(arg("--add-fasta-dialog"))
            if (arg("--add-reviewed-dialog")) { const r = JSON.parse(arg("--add-reviewed-dialog")); window.documentAt(tabs.currentIndex).addReviewed(r.sequence, r.name, r.mafft !== false) }
            if (arg("--fit")) c.fit()
            if (arg("--sanger-position")) c.press(Number(arg("--sanger-position")), 40)
            if (arg("--sanger-context")) {
                const point = arg("--sanger-context").split(",").map(Number)
                window.documentAt(tabs.currentIndex).maskMenuAt(point[0], point[1])
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
                    else if (cmd === "fasttree") c.run_form("fasttree", "tree", a || "{}")
                    else if (cmd === "trimal") c.trim_alignment()
                    else if (cmd === "iqtree") c.run_form("iqtree", "tree", a || "{}")
                    else if (cmd === "distance") c.run_form("distance", "nj", JSON.stringify({model: a || "k2p"}))
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
            if (arg("--extract")) {
                const completed = function(path) {
                    c.extractionFinished.disconnect(completed)
                    console.log("extracted:", path, "|", c.message)
                }
                c.extractionFinished.connect(completed)
                if (!c.extract_selection()) {
                    c.extractionFinished.disconnect(completed)
                    console.log("extraction rejected:", c.message)
                }
            }
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
            if (arg("--distance-action")) c.run_form("distance", arg("--distance-action"), arg("--distance-options") || "{}")
            if (arg("--species-dialog")) window.beginDelimitation(arg("--species-dialog"))
            if (arg("--distance-dialog")) toolSettings.show(c, "distance", arg("--distance-dialog"))
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
        onTriggered: { window.documentAt(tabs.currentIndex).showAdditionChoices(); shot.start() }
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
                // Every child, so a width that does not add up can be traced.
                const children = []
                for (const child of ribbon.currentPage.children)
                    children.push({name: child.title || String(child).split("(")[0], visible: child.visible,
                        x: child.x, width: child.width, implicitWidth: child.implicitWidth})
                console.log("RIBBON_AUDIT", JSON.stringify({page: window.ribbonTab,
                    height: ribbon.height, contentWidth: ribbon.contentWidth,
                    viewportWidth: ribbon.viewportWidth, pageWidth: ribbon.pageWidth,
                    pageImplicitWidth: ribbon.currentPage.implicitWidth, spacing: ribbon.currentPage.spacing,
                    groups: groups, children: children}))
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
