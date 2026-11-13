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
    // Set by tools/ScriptedRun.qml, the check tools' window: no first-launch form.
    property bool scriptedRun: false
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

    // Closing the window (or Ctrl+Q) asks about unsaved changes first; the
    // open documents decide what to ask and in which order to save.
    property bool closeConfirmed: false
    onClosing: (close) => {
        if (closeConfirmed) { documentOpens.shutdown(); return }
        const step = JSON.parse(documentOpens.quit())
        if (step.leave) { leave(step.leave); documentOpens.shutdown(); return }
        close.accepted = false
        quitStep(step)
    }
    // Delete the empty species workspaces the quit step names.
    function leave(drafts) {
        for (const token of drafts) {
            const d = documentAt(documentOpens.index_of(token))
            if (d) d.controller.discard_empty_workspace()
        }
    }
    function quitStep(step) {
        if (step.ask) quitDialog.dialog().ask(step.ask, step.error)
        else if (step.save) saveTarget(documentTarget(documentOpens.target_index(step.save.token, step.save.path)), "",
            success => quitStep(JSON.parse(documentOpens.quit_saved(success))))
        else if (step.refuse) preferences.project_error = step.refuse
        else if (step.choose) chooseProject(step.choose, step.open)
        else if (step.leave && !step.project) { leave(step.leave); closeConfirmed = true; window.close() }
        else if (step.leave) { // A switch: every tab closes, ending its session at once; then the project is chosen.
            leave(step.leave)
            while (documentOpens.count > 0) removeDocument(documentOpens.count - 1)
            chooseProject(step.project, step.open); refreshCurrent()
        }
    }
    // A switch made to open a file in its project (D16) carries the file.
    function chooseProject(folder, file) {
        preferences.project_error = shell.choose_project(folder)
        if (file && !preferences.project_error.length) Qt.callLater(() => openPath(file.path, true))
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
    GenomeController { id: shell; objectName: "shell" }

    readonly property var ribbonNavigation: ribbonHints
    RibbonKeyHints {
        id: ribbonHints
        parent: window.contentItem
        anchors.fill: parent
        z: 1000
        hostWindow: window
        tabBar: ribbonTabBar
        ribbon: ribbon
    }
    // Open files, one tab each: the rows of documentOpens.
    function documentAt(index) {
        if (index < 0 || index >= documentOpens.count) return null
        const slot = docs.itemAt(index)
        return slot && slot.adoptedSlot === true ? (slot.ready ? slot.documentView : null) : slot
    }
    // Consent belongs to a tab lifetime and owner, never its shifting row.
    function documentTarget(index) {
        const document = documentAt(index)
        if (!document) return null
        return {token: documentOpens.token_at(index), controller: document.controller, path: documentOpens.path_at(index)}
    }
    // The tab's session ends now, not when Qt later deletes the tab, so a
    // project switch right after sees the project free.
    function removeDocument(index) {
        const removed = documentAt(index)
        if (removed && removed.controller) removed.controller.close_session()
        const applying = applyingOpenFocus
        applyingOpenFocus = true
        documentOpens.remove_tab(index)
        applyingOpenFocus = applying
    }
    property bool applyingOpenFocus: false
    onActiveFocusItemChanged: if (!applyingOpenFocus) documentOpens.revoke_focus()
    Component.onDestruction: documentOpens.shutdown()
    DocumentOpens {
        id: documentOpens
        onAcquired: (token, kind) => {
            const index = documentOpens.index_of(token)
            const slot = index >= 0 ? docs.itemAt(index) : null
            if (slot && slot.adoptedSlot === true) slot.construct(kind)
            else documentOpens.retire(token)
        }
        onFailed: (token, message) => {
            const index = documentOpens.index_of(token)
            if (index < 0) return
            const path = documentOpens.path_at(index)
            window.removeDocument(index)
            provenance.notification(path, message, "fail")
        }
        onActivated: (token, installedPath) => {
            const index = documentOpens.index_of(token)
            const document = index >= 0 ? window.documentAt(index) : null
            if (!document || !document.controller || document.controller.file_path !== installedPath) return
            window.applyingOpenFocus = true
            tabs.currentIndex = index
            window.focusOpenedDocument = true
            window.refreshCurrent()
            window.applyingOpenFocus = false
        }
    }

    // The active tab's controller and view, or null.
    property var currentDocument: null
    property bool projectingDocument: false
    property var current: null
    onCurrentChanged: alphabetQuestion.dialog().controller = current
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
    readonly property var ribbonTabs: commandList.ribbon_tabs(currentKind)
    onRibbonTabsChanged: if (!projectingDocument) ribbonTabName = commandList.ribbon_tab(currentKind, ribbonTabName)
    readonly property string ribbonTab: ribbonTabs.includes(ribbonTabName) ? ribbonTabName : "File"
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
    // Another project closes the open documents first, asking as quitting does.
    // A task still running in the old project, from a tab or not, refuses it first.
    function selectProject(path, then = "") {
        const chosen = preferences.select_project(path)
        if (chosen.length) quitStep(JSON.parse(documentOpens.switch_project(chosen, shell.project_dir, then)))
    }
    property bool preferencesReady: false
    AppSettings {
        id: preferences
        objectName: "preferences"
        onSnapshotChanged: if (window.preferencesReady) window.loadPreferences()
        onRelease_available: notice => provenance.notification("", notice, "notify")
        onErrorChanged: if (error.length && window.preferencesReady) {
            preferencesError.dialog().open()
        }
    }
    function persistPreference(key, value) {
        if (preferencesReady) preferences.update(key, JSON.stringify(value))
    }
    OnDemand { id: preferencesError; objectName: "preferencesError"; sourceComponent: Component { AppContentDialog {
        id: preferencesErrorItem
        title: "Preferences could not be loaded or saved"
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
                    if (!preferences.error.length) preferencesErrorItem.close()
                }
            }
        }
    } } }
    function loadPreferences() {
        preferencesReady = false
        if (!preferences.reload()) return
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
        return shell.local_file_url(preferences.dialog_folder(shell.project_dir, currentPath, current ? current.export_path || current.file_path || "" : ""))
    }
    function projectPickerFolder() { return shell.local_file_url(preferences.project_picker_directory()) }
    property var projectDialog: null
    function openProjectPicker() {
        if (projectDialog && projectDialog.visible) return
        // Qt applies FolderDialog.currentFolder to the backend on its first
        // show only. A fresh native dialog makes every opening use our policy.
        projectDialog = projectDialogComponent.createObject(window, {currentFolder: projectPickerFolder()})
        if (projectDialog) projectDialog.open()
    }


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
        ribbonTabName = commandList.ribbon_tab(currentKind, ribbonTabName)
        if (allowFocus && currentView && (focusOpenedDocument || !tabs.activeFocus)) {
            // Background model updates must not steal focus from controls/dialogs.
            if (focusOpenedDocument || !window.activeFocusItem || !window.activeFocusItem.visible)
                currentView.forceActiveFocus(Qt.OtherFocusReason)
        }
        if (currentView) focusOpenedDocument = false
    }

    function openTextRepresentation() {
        const source = current
        if (!currentView || !source || typeof source.accessible_page !== "function") return
        textRepresentation.dialog().source = source
        textRepresentation.dialog().open()
    }
    OnDemand { id: alphabetQuestion; objectName: "alphabetQuestion"; sourceComponent: Component { AlphabetDialog {} } }
    OnDemand { id: readOnlyCopy; objectName: "readOnlyCopy"; sourceComponent: Component { ReadOnlyCopyDialog { onSaveCopy: refused => window.saveCopy(refused) } } }
    OnDemand { id: otherProject; objectName: "otherProject"; sourceComponent: Component { AppContentDialog {
        id: otherProjectItem
        objectName: "otherProjectDialog"
        property var file: null
        title: "File in another project"
        standardButtons: Dialog.Cancel
        initialFocusItem: switchProject
        body: ColumnLayout {
            Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: otherProjectItem.file ? "This file is in project " + otherProjectItem.file.name + ". Switch to that project, or open it on its own?" : "" }
            RowLayout {
                AppButton { id: switchProject; objectName: "switchToProject"; text: "&Switch project"; onClicked: { otherProjectItem.close(); window.selectProject(otherProjectItem.file.root, JSON.stringify(otherProjectItem.file)) } }
                AppButton { objectName: "openOnItsOwn"; text: "&Open on its own"; onClicked: { otherProjectItem.close(); window.openPath(otherProjectItem.file.path, true) } }
            }
        }
    } } }
    OnDemand { id: labelPlacement; objectName: "labelPlacement"; sourceComponent: Component { LabelPlacementDialog {} } }
    OnDemand { id: textRepresentation; objectName: "textRepresentation"; sourceComponent: Component { TextRepresentationDialog { id: textRepresentationItem } } }

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
    function openPath(path, onItsOwn = false) {
        const step = JSON.parse(documentOpens.open(path, onItsOwn))
        if (step.other_project) { otherProject.dialog().file = step.other_project; otherProject.dialog().open(); return }
        if (step.error) { provenance.notification(path, step.error, "fail"); return }
        if (step.project) {
            // Project documents are edited on the Export tab.
            documentOpens.revoke_focus()
            ribbonTabName = "Export"
            if (step.project === "metadata") specimenTable.dialog().open()
            else submissions.open(path)
            return
        }
        const index = step.new !== undefined ? step.new : step.switch
        if (!step.view) {
            // Opened on a worker: the tab shows once the document arrives.
            applyingOpenFocus = true
            tabs.currentIndex = index
            if (step.activate) refreshCurrent(false)
            applyingOpenFocus = false
            if (step.activate) documentOpens.activate(step.token)
            return
        }
        documentOpens.revoke_focus()
        focusOpenedDocument = true
        tabs.currentIndex = index
        if (step.switch !== undefined) Qt.callLater(refreshCurrent)
    }

    // The controller the species dialog was opened for; the open documents
    // decide whether its request runs, and open the workspace it creates.
    property var delimitationSource: null
    function beginDelimitation(method) {
        if (!current || !documentOpens.species_request_allowed(currentKind, current.busy, method, !!current.linked_alignment)) return
        delimitationSource = current
        documentOpens.begin_delimitation(tabs.currentIndex)
        speciesDialog.dialog().engineProblems = method === "ptp" ? JSON.parse(shell.delimitation_engines()) : ({})
        speciesDialog.dialog().show(method, alignmentOpen ? current.file_path : current.linked_alignment || "",
            treeOpen ? current.file_path : delimitationOpen ? current.display_path : "")
    }
    function requestDelimitation(method, input, rooted, settings, displayMethod) {
        const source = delimitationSource
        const step = documentOpens.request_delimitation(source ? source.file_path : "", !source || source.busy,
            JSON.stringify({method: method, input: input, rooted: rooted, settings: settings, display: displayMethod}))
        if (step === "create") { if (!source.create_delimitation(method)) documentOpens.creation_refused() }
        else if (step.length) documentAt(documentOpens.index_of(step)).controller.start_analysis(method, input, rooted, settings, displayMethod)
    }
    Connections {
        target: window.delimitationSource
        ignoreUnknownSignals: true
        function onWorkspaceCreated(path) {
            const reply = documentOpens.workspace_created(window.delimitationSource.file_path, path)
            if (!reply.length) return
            const run = JSON.parse(reply)
            Qt.callLater(() => {
                const document = window.documentAt(documentOpens.index_of(run.token))
                if (document) document.controller.start_analysis(run.method, run.uses_open_alignment ? document.controller.linked_alignment : run.input,
                    run.rooted, run.settings, run.display)
            })
        }
    }
    OnDemand { id: speciesDialog; sourceComponent: Component { DelimitationAnalysisDialog {
        id: speciesDialogItem
        pathResolver: shell
        workers: window.localCpuThreads
        onRequested: (method, input, rooted, settings, displayMethod) =>
            window.requestDelimitation(method, input, rooted, settings, displayMethod)
    } } }
    DelimitationController { id: combineController }
    OnDemand { id: combinePicker; sourceComponent: Component { FileDialog {
        id: combinePickerItem
        title: "Combine saved species hypotheses"
        fileMode: FileDialog.OpenFiles
        nameFilters: ["Species hypotheses (*.spart *.spartxml *.spart-xml *.xml)"]
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        readonly property var paths: Array.from(selectedFiles, url => shell.local_file_path(url))
        onAccepted: {
            if (selectedFiles.length < 2) { combineError.dialog().message = "Choose at least two species hypothesis files."; combineError.dialog().open(); return }
            const candidates = JSON.parse(combineController.combine_candidates(JSON.stringify(paths)))
            if (!candidates.length) { combineError.dialog().message = combineController.message; combineError.dialog().open(); return }
            combineChoice.dialog().show(candidates)
        }
    } } }

    OnDemand { id: combineChoice; sourceComponent: Component { CombineHypothesesDialog {
        id: combineChoiceItem
        property string selection: ""
        onChosenHypotheses: chosen => {
            selection = chosen
            // Suggest <shared root>.combinedNN.spart beside the files just chosen.
            const paths = combinePicker.dialog().paths
            const suggested = combineController.combine_name(JSON.stringify(paths))
            combinedDestination.dialog().currentFolder = shell.parent_folder_url(suggested.length ? suggested : paths[paths.length - 1])
            if (suggested.length) combinedDestination.dialog().selectedFile = shell.local_file_url(suggested)
            combinedDestination.dialog().open()
        }
    } } }

    OnDemand { id: combinedDestination; sourceComponent: Component { FileDialog {
        id: combinedDestinationItem
        title: "Save combined species hypotheses as a new file"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "spart"
        nameFilters: ["Species hypotheses (*.spart)"]
        onAccepted: {
            const output = shell.local_file_path(selectedFile)
            if (combineController.combine_files(JSON.stringify(combinePicker.dialog().paths), output, combineChoice.dialog().selection)) window.openPath(output)
            else { combineError.dialog().message = combineController.message; combineError.dialog().open() }
        }
    } } }

    OnDemand { id: combineError; sourceComponent: Component { AppContentDialog {
        id: combineErrorItem
        property string message: ""
        title: "Could not combine hypotheses"
        standardButtons: Dialog.Close
        body: AppErrorLabel { text: combineErrorItem.message; wrapMode: Text.Wrap }
    } } }

    function startTraceReview(paths, reference, target, cutoff = 20, forceNew = false) {
        const tab = documentOpens.trace_review(paths[0], forceNew)
        if (tab >= 0) { tabs.currentIndex = tab; return }
        if (tab === -1) tabs.currentIndex = documentOpens.add_tab(paths[0], "trace")
        else window.openPath(paths[0])
        // The review starts in this tab's document even if the user moves on.
        const traceDocument = window.documentAt(tabs.currentIndex)
        Qt.callLater(() => { if (traceDocument) traceDocument.startReview(paths, reference, target, cutoff) })
    }

    Connections {
        target: window.current
        ignoreUnknownSignals: true
        function onLabel_questionChanged() { labelPlacement.dialog().controller = window.current }
        function onRefused_editChanged() { readOnlyCopy.dialog().owner = window.current }
        function onAlphabet_questionChanged() { alphabetQuestion.dialog().controller = window.current }
        function onTaxaSelected(names) {
            for (let i = 0; i < docs.count; i++) {
                const d = window.documentAt(i)
                if (d) documentOpens.link_taxa(window.current, d.controller, names)
            }
        }
    }

    // A species workspace created for an analysis that produced nothing is
    // removed on close rather than left as an empty file beside the data.
    function closeTab(i) {
        documentOpens.revoke_focus()
        const step = JSON.parse(documentOpens.close(i))
        if (step.ask) { confirmClose.dialog().target = documentTarget(i); confirmClose.dialog().saveError = ""; confirmClose.dialog().open(); return }
        if (step.draft) window.documentAt(i).controller.discard_empty_workspace()
        window.removeDocument(i)
        Qt.callLater(refreshCurrent)
    }

    // Clipboard access goes through a hidden text edit.
    TextEdit {
        id: clipboard
        visible: false
        function put(text) { clipboard.text = text; clipboard.selectAll(); clipboard.copy() }
    }

    Command {
        id: installedOperationsCommand
        commandId: "tools.installed-operations"
        iconName: "more"
        enabled: window.current !== null && window.currentPath.length > 0
        onTriggered: operationWindow.dialog().show(shell.project_dir.length ? shell.project_dir : shell.local_file_path(shell.parent_folder_url(window.currentPath)), window.currentPath, !!window.current.dirty)
    }
    Command {
        id: newSequenceCommand
        commandId: "file.new-sequence"
        iconName: "new-file"
        defaultShortcut: "Ctrl+N"
        // D21: a new document's recovery lives in the chosen project.
        enabled: shell.project_dir.length > 0
        onTriggered: newSequenceDialog.dialog().prepare("", "")
    }
    Command {
        id: addAnnotationCommand
        commandId: "ribbon.annotate-the-selection"
        iconName: "annotation-add"
        defaultShortcut: "Ctrl+Shift+N"
        enabled: window.currentKind === "sequence" && window.current !== null && !window.current.busy
            && !(window.activeFocusItem && window.activeFocusItem.cursorPosition !== undefined)
        onTriggered: editor.openNew()
    }
    property var pendingSequence: null
    OnDemand { id: newSequenceDialog; sourceComponent: Component { AddSequenceDialog {
        id: newSequenceDialogItem
        controller: shell
        newDocument: true
        onInputError: message => validationMessage = message
        onRejected: {
            const pending = window.pendingSequence
            window.pendingSequence = null
            if (!pending) return
            const pendingIndex = documentOpens.index_of(pending.token)
            if (pendingIndex >= 0) window.removeDocument(pendingIndex)
            tabs.currentIndex = pending.previous
        }
        onCreateRequested: (name, sequence) => {
            if (window.pendingSequence) return
            const error = shell.new_sequence_error(name, sequence)
            if (error.length) { validationMessage = error; return }
            const index = documentOpens.new_sequence(name)
            window.pendingSequence = {path: documentOpens.path_at(index), name: name, sequence: sequence, previous: tabs.currentIndex, token: documentOpens.token_at(index)}
            tabs.currentIndex = index
        }
    } } }
    function initializeSequence(document) {
        const pending = window.pendingSequence
        if (!pending || pending.path !== document.path) return
        const error = document.controller.create_sequence(pending.name, pending.sequence)
        window.pendingSequence = null
        if (error.length) {
            window.removeDocument(document.index)
            tabs.currentIndex = pending.previous
            newSequenceDialog.dialog().validationMessage = error
            return
        }
        newSequenceDialog.dialog().accept()
        Qt.callLater(() => { window.refreshCurrent(); document.view.forceActiveFocus() })
    }
    Command {
        id: openCommand
        commandId: "file.open"
        iconName: "open"
        defaultShortcut: "Ctrl+O"
        onTriggered: openDialog.dialog().open()
    }
    Command {
        id: saveCommand
        commandId: "file.save"
        iconName: "save"
        defaultShortcut: "Ctrl+S"
        enabled: window.current !== null && (!window.traceOpen || window.current.review_mode)
        onTriggered: window.traceOpen ? window.traceDocument.saveReview() : window.saveTarget(window.documentTarget(tabs.currentIndex), "", null)
    }
    Command {
        id: saveAsCommand
        commandId: "file.save-as"
        iconName: "save-as"
        defaultShortcut: "Ctrl+Shift+S"
        enabled: window.current !== null
        onTriggered: window.saveAs()
    }
    Command {
        id: projectCommand
        commandId: "project.open"
        iconName: "project"
        onTriggered: window.openProjectPicker()
    }
    Command {
        id: shortcutsCommand
        commandId: "help.shortcuts"
        iconName: "keyboard"
        onTriggered: shortcutsWindow.dialog().show("")
    }
    Command {
        id: aboutCommand
        commandId: "help.about"
        iconName: "info"
        onTriggered: about.dialog().open()
    }

    Command {
        id: preferencesCommand
        commandId: "application.preferences"
        iconName: "settings"
        onTriggered: preferencesWindow.dialog().open()
    }
    Command {
        id: findCommand
        commandId: "view.find"
        iconName: "search"
        defaultShortcut: "Ctrl+F"
        enabled: window.current !== null
        onTriggered: {
            const document = window.documentAt(tabs.currentIndex)
            if (document && document.openFind) document.openFind()
        }
    }
    Command {
        id: textRepresentationCommand
        commandId: "view.text"
        iconName: "text-representation"
        defaultShortcut: "Ctrl+Alt+T"
        enabled: window.current !== null
        onTriggered: window.openTextRepresentation()
    }
    Command {
        id: contextShortcutsCommand
        commandId: "help.context-shortcuts"
        iconName: "keyboard"
        onTriggered: shortcutsWindow.dialog().show(window.ribbonTab)
    }
    Command {
        id: learnCommand
        commandId: "help.learn"
        iconName: "book"
    }
    Command {
        id: exportSvgCommand
        commandId: "ribbon.export-an-editable-svg-of-this-view"
        iconName: "save-as"
        enabled: window.current !== null
        onTriggered: window.exportSvg()
    }
    Command {
        id: exportPngCommand
        commandId: "figure.export-png"
        iconName: "figure-export"
        enabled: window.current !== null
        onTriggered: window.exportSvg("png")
    }
    AppProvenance {
        id: provenance
        onMessageChanged: if (message.length && message !== "notify: Working…") notification("", message, message.startsWith("fail:") ? "fail" : "done")
    }
    AppJobs { id: jobService }
    property string frozenReport: ""
    OnDemand { id: documentRecord; sourceComponent: Component { DocumentRecordDialog {
        id: documentRecordItem
        service: provenance
        documentPath: window.currentPath
        controller: window.current
        onCopyText: text => clipboard.put(text)
        onExportReport: snapshot => { window.frozenReport = snapshot; reportFolder.dialog().open() }
        onVerify: verifyProvenanceCommand.trigger()
        onPackageRecord: packageCommand.trigger()
        onOpenRecordFolder: showProvenanceCommand.trigger()
    } } }

    OnDemand { id: reportFolder; sourceComponent: Component { FolderDialog {
        id: reportFolderItem
        title: "Export the selected revision's report"
        onAccepted: { provenance.export_report(window.frozenReport, shell.local_file_path(selectedFolder)); provenanceDialog.dialog().open() }
    } } }

    OnDemand {
        id: tasksWindow
        sourceComponent: Component {
            TasksDialog {
                id: tasksDialog
                service: provenance
                jobs: jobService
                documents: documentOpens
                onOpenDocument: path => window.openPath(path)
                onCopyText: text => clipboard.put(text)
            }
        }
    }
    readonly property string savedRecordPath: window.traceOpen && window.current ? window.current.file_path : window.currentPath
    property string provenanceSource: ""
    property string provenanceExport: ""
    function inspectProvenance(operation) {
        provenanceExport = ""
        provenanceDialog.dialog().open()
        provenance.start(operation, window.savedRecordPath, "")
    }
    function exportProvenance(operation) {
        provenanceSource = window.savedRecordPath
        provenanceExport = operation
        provenanceFolder.dialog().open()
    }
    Command {
        id: historyCommand
        commandId: "provenance.history"
        iconName: "history"
        enabled: window.current !== null && !provenance.busy
        onTriggered: documentRecord.dialog().showPage(0)
    }
    Command {
        id: methodsCommand
        disabledReason: tooltip
        commandId: "provenance.methods"
        iconName: "methods-export"
        tooltip: window.current === null ? "Open a document to read its methods" : provenance.busy ? "Wait for the current provenance operation" : "Read methods for the current document, including unsaved changes"
        enabled: window.current !== null && !provenance.busy
        onTriggered: { documentRecord.dialog().savedRevision = false; documentRecord.dialog().showPage(1) }
    }
    Command {
        id: showProvenanceCommand
        commandId: "provenance.show-files"
        iconName: "open"
        enabled: window.current !== null && !provenance.busy
        onTriggered: {
            const folder = provenance.bundle_path(window.savedRecordPath)
            if (folder.length) Qt.openUrlExternally(shell.local_file_url(folder))
            else provenanceDialog.dialog().open()
        }
    }
    Command {
        id: verifyProvenanceCommand
        commandId: "provenance.verify"
        enabled: window.current !== null && !provenance.busy
        onTriggered: window.inspectProvenance("verify")
    }
    Command {
        id: packageCommand
        commandId: "provenance.package"
        enabled: window.current !== null && !provenance.busy
        onTriggered: window.exportProvenance("package")
    }
    Command {
        id: recoverCommand
        commandId: "file.recover-work"
        iconName: "open"
        enabled: !provenance.busy
        onTriggered: {
            recoveryPicker.dialog().sessions = JSON.parse(provenance.recovery_sessions())
            recoveryPicker.dialog().open()
        }
    }
    OnDemand { id: recoveryPicker; sourceComponent: Component { AppFormDialog {
        id: recoveryPickerItem
        title: "Recover unsaved work"
        standardButtons: Dialog.Close
        property var sessions: []
        initialFocusItem: recoveryChoice
        Label { text: "Recovery creates copies in a new directory using the unchanged original or a retained recovery file. Originals and journals are preserved."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { visible: !recoveryPickerItem.sessions.length; text: provenance.message.startsWith("fail:") ? provenance.message : "No recovery sessions are available."; wrapMode: Text.Wrap; Layout.fillWidth: true }
        AppComboBox {
            id: recoveryChoice
            Kirigami.FormData.label: "Session:"
            Layout.fillWidth: true
            model: recoveryPickerItem.sessions.map(session => session.name + " — " + session.id)
            enabled: model.length > 0
        }
        AppButton {
            text: "Choose recovery destination…"
            enabled: recoveryChoice.currentIndex >= 0 && recoveryPickerItem.sessions.length > 0
            onClicked: {
                window.provenanceSource = recoveryPickerItem.sessions[recoveryChoice.currentIndex].path
                window.provenanceExport = "recover"
                recoveryPickerItem.close()
                provenanceFolder.dialog().open()
            }
        }
    } } }

    OnDemand { id: provenanceFolder; sourceComponent: Component { FolderDialog {
        id: provenanceFolderItem
        title: window.provenanceExport === "methods" ? "Choose the folder for generated reports" : window.provenanceExport === "recover" ? "Choose the parent folder for recovered copies" : "Choose the parent folder for a new reproduction package"
        onAccepted: {
            let destination = shell.local_file_path(selectedFolder)
            if (window.provenanceExport === "package") destination += "/" + window.fileName(window.provenanceSource) + ".reproduction"
            if (window.provenanceExport === "recover") destination += "/" + window.fileName(window.provenanceSource) + ".recovered"
            provenanceDialog.dialog().open()
            provenance.start(window.provenanceExport, window.provenanceSource, destination)
        }
    } } }

    OnDemand { id: provenanceDialog; sourceComponent: Component { AppDialog {
        id: provenanceDialogItem
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
    } } }
    OnDemand { id: preferencesWindow; sourceComponent: Component { PreferencesDialog { id: preferencesDialog; settings: preferences; commands: window.applicationCommands; defaultFontSizePoints: window.defaultFontSizePoints; onContactDetailsRequested: window.showContacts() } } }

    // The one list of commands: keys, scopes, icons and why one is unavailable.
    AppCommands { id: commandList }
    readonly property string commandContext: JSON.stringify({project: shell.project_dir.length > 0, document: current !== null,
        path: currentPath.length > 0, kind: currentKind, busy: !!current && !!current.busy, calling: !!current && !!current.calling,
        linked_alignment: !!current && !!current.linked_alignment, species_view: speciesView !== null, records_busy: provenance.busy,
        submission_busy: submissionJobs.busy, trace_working: traceOpen && !!traceDocument && traceDocument.working,
        review_mode: !!current && !!current.review_mode, unresolved: !!current && current.unresolved > 0})
    component Command: AppCommand {
        text: commandList.caption(commandId)
        tooltip: commandList.tip(commandId)
        customization: window.customization
        disabledReason: commandList.disabled_reason(commandId, window.commandContext)
        hintScope: commandList.scope(commandId)
        defaultKeyHint: commandList.key(commandId)
    }
    component RibbonButton: AppCommandButton {
        text: commandList.caption(commandId)
        tip: commandList.tip(commandId)
        customization: window.customization
        disabledReason: commandList.disabled_reason(commandId, window.commandContext)
        hintScope: commandList.scope(commandId)
        defaultKeyHint: commandList.key(commandId)
        enabled: window.current !== null
        iconName: commandList.icon(commandId)
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
        RibbonButton { commandId: "species.gmyc-" + speciesMethods.commandSuffix; iconName: "tree-gmyc"; enabled: speciesMethods.canRun; onInvoked: window.beginDelimitation("gmyc") }
        RibbonButton { commandId: "species.asap-" + speciesMethods.commandSuffix; iconName: "matrix-asap"; enabled: (window.alignmentOpen || (window.delimitationOpen && window.current && window.current.linked_alignment.length > 0)) && speciesMethods.canRun; onInvoked: window.beginDelimitation("asap") }
        RibbonButton { commandId: "species.ptp-" + speciesMethods.commandSuffix; iconName: "phylogram-ptp"; enabled: speciesMethods.canRun; onInvoked: window.beginDelimitation("ptp") }
        RibbonButton { commandId: "species.manual-" + speciesMethods.commandSuffix; iconName: "hypothesis-manual"; enabled: speciesMethods.canRun; onInvoked: window.beginDelimitation("manual") }
        RibbonButton { commandId: "species.combine-" + speciesMethods.commandSuffix; iconName: "hypothesis-combine"; onInvoked: combinePicker.dialog().open() }
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
            AppButton { text: "Details…"; onClicked: preferencesError.dialog().open() }
        }
        Label {
            visible: preferences.project_error.length > 0
            Layout.fillWidth: true
            text: preferences.project_error
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
                    objectName: "ribbonTabBar"
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
                    text: jobService.running ? "Tasks (" + jobService.running + ")" : "Tasks" + (jobService.failed ? " · " + jobService.failed + " failed" : "")
                    Accessible.name: "Tasks and notifications, " + jobService.running + " active, " + jobService.failed + " failed"
                    onClicked: tasksWindow.dialog().open()
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
            currentIndex: commandList.ribbon_panel(window.ribbonTab)
            background: Rectangle { color: Qt.alpha(shell.foreground, 0.025) }

                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        title: "Project"
                        AppRibbonButton { action: projectCommand; visible: projectCommand.visible; text: commandList.ribbon_label(projectCommand.commandId); iconName: "project"; tip: projectCommand.effectiveTooltip }
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
                                RibbonMenuItem { sizeFactor: window.ribbonSizeFactor; text: "No recent projects"; enabled: false; visible: preferences.recent_projects.length === 0 }
                                Repeater {
                                    model: preferences.recent_projects
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
                        RibbonButton { commandId: "species.combine-project"; iconName: "hypothesis-combine"; enabled: true; onInvoked: combinePicker.dialog().open() }
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
                        RibbonButton { commandId: "ribbon.zoom-out"; iconName: "zoom-out"; keys: "Ctrl+−"; onInvoked: (window.delimitationOpen ? window.speciesTree : window.current).zoom_out() }
                        RibbonButton { commandId: "ribbon.zoom-in"; iconName: "zoom-in"; keys: "Ctrl++"; onInvoked: (window.delimitationOpen ? window.speciesTree : window.current).zoom_in() }
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
                        AppCheckBox {
                            objectName: "colorBlindFriendly"
                            property string keyHint: "CB"
                            text: "Colour-blind colours"
                            HoverHandler { id: paletteTipHover }
                            Accessible.description: ToolTip.text
                            ToolTip.text: "Data colours that stay distinct for colour-blind readers, in the light or dark variant of the appearance. Labels still identify the data."
                            ToolTip.visible: paletteTipHover.hovered || activeFocus
                            ToolTip.delay: 400
                            checked: !!window.preferenceValues.ui && window.preferenceValues.ui.data_color_scheme === "color-blind-friendly"
                            focusPolicy: Qt.StrongFocus
                            onToggled: window.persistPreference("colorBlindFriendly", checked)
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
                        AppRibbonButton { action: addAnnotationCommand; text: commandList.ribbon_label(addAnnotationCommand.commandId); visible: addAnnotationCommand.visible; iconName: "annotation-add"; tip: addAnnotationCommand.effectiveTooltip }
                        RibbonButton { commandId: "ribbon.edit-the-selected-annotation"; iconName: "annotation-edit"; keys: "Enter"; onInvoked: editor.openSelected() }
                        RibbonButton { commandId: "ribbon.delete-the-selected-annotation";
                            iconName: "annotation-delete"
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
                        RibbonButton { commandId: "ribbon.select-the-previous-annotation"; iconName: "left"; onInvoked: window.current.step_feature(false) }
                        RibbonButton { commandId: "ribbon.select-the-next-annotation"; iconName: "right"; onInvoked: window.current.step_feature(true) }
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
                        RibbonButton { commandId: "sequence.blast"; iconName: "search"; onInvoked: window.beginBlast("whole") }
                        RibbonButton { commandId: "ribbon.annotate-open-reading-frames";
                            text: "ORFs"
                            tip: "Annotate open reading frames"
                            enabled: window.current && !window.current.busy
                            onInvoked: orfDialog.dialog().search(false)
                        }
                        RibbonButton { commandId: "ribbon.annotate-protein-coding-genes-with-prodigal";
                            text: "Prodigal"
                            tip: "Annotate protein-coding genes with Prodigal"
                            enabled: window.current && !window.current.busy
                            onInvoked: genesDialog.dialog().predict(false)
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
                        RibbonButton { commandId: "ribbon.move-the-selected-rows-up"; iconName: "up"; keys: "Alt+↑"; onInvoked: window.current.move_rows(true) }
                        RibbonButton { commandId: "ribbon.move-the-selected-rows-down"; iconName: "down"; keys: "Alt+↓"; onInvoked: window.current.move_rows(false) }
                        RibbonButton { commandId: "ribbon.rename-the-cursor-s-row"; Accessible.name: tip; keys: "F2"; onInvoked: renameDialog.dialog().askRow() }
                        RibbonButton { commandId: "ribbon.delete-the-selected-rows"; iconName: "delete"; onInvoked: window.current.remove_rows() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        disabledReason: "Open an alignment to use this group"
                        enabled: window.alignmentOpen
                        title: "Find"
                        RibbonButton { commandId: "alignment.blast"; iconName: "search"; onInvoked: window.beginBlast("all") }
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
                        RibbonButton { commandId: "ribbon.type-a-new-sequence-or-paste-fasta"; iconName: "new-file"; enabled: window.current !== null && !window.current.busy; onInvoked: window.documentAt(tabs.currentIndex).addText() }
                        RibbonButton { commandId: "ribbon.add-all-sequences-from-a-fasta-file"; iconName: "add-file"; enabled: window.current !== null && !window.current.busy; onInvoked: window.documentAt(tabs.currentIndex).addFasta() }
                        RibbonButton { commandId: "ribbon.review-chromatograms-and-add-their-consensus"; iconName: "waveform"; enabled: window.current !== null && !window.current.busy; onInvoked: window.documentAt(tabs.currentIndex).addTraces() }
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
                        captions: commandList
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
                            iconName: "tree-clock"
                            enabled: window.treeOpen && window.current !== null && window.currentPath.length > 0
                            onInvoked: operationWindow.dialog().showOperation(shell.project_dir.length ? shell.project_dir : shell.local_file_path(shell.parent_folder_url(window.currentPath)), window.currentPath, !!window.current.dirty, "basecraft.sanderson/date_tree@1")
                        }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    TreeCladeGroup {
                        controller: window.treeOpen ? window.current : null
                        captions: commandList
                        customization: window.customization
                        onAnnotateRequested: cladeEditor.openSelected()
                        Component.onCompleted: commands.forEach(command => window.registerRibbonCommand(command))
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    TreeStyleGroup {
                        controller: window.treeOpen ? window.current : null
                        customization: window.customization
                        onExtraRequested: treeStyle.dialog().openStyle("extra")
                        onLabelsRequested: metadataTipLabels.dialog().show(window.current, window.currentPath)
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
                        RibbonButton { commandId: "ribbon.add-support-values-from-another-analysis-of-the-same"; onInvoked: supportFile.dialog().open() }
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
                        RibbonButton { commandId: "ribbon.which-support-values-show-circles-at-nodes-sizes"; iconName: "support-style"; enabled: supportGroup.hasSupport; onInvoked: treeStyle.dialog().openStyle("support") }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    TreeAlignmentGroup {
                        controller: window.treeOpen ? window.current : null
                        captions: commandList
                        customization: window.customization
                        onLinkRequested: (target) => linkAlignmentFile.dialog().ask(target)
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
                                onInvoked: toolSettings.dialog().show(window.current, "distance", modelData.value)
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
                            onInvoked: toolSettings.dialog().show(window.current, "fasttree", "tree")
                        }
                        RibbonButton { commandId: "ribbon.select-a-model-and-infer-a-maximum-likelihood-tree";
                            iconName: "tree-infer"
                            text: "IQ-TREE"
                            enabled: window.current !== null && !window.current.busy
                            tip: "Infer a maximum-likelihood tree: choose the model, rate variation and branch support"
                            onInvoked: toolSettings.dialog().show(window.current, "iqtree", "tree")
                        }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }
                RibbonPage {
                    SpeciesMethodsGroup { commandSuffix: "workspace" }
                    RibbonGroup {
                        title: "Specimens"
                        RibbonButton { commandId: "species.roster-links"; iconName: "matrix"; enabled: window.speciesView !== null && specimenMetadata.available; onInvoked: rosterConsumer.dialog().show(window.current, window.currentPath) }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Display"
                        ModeButton { commandId: "species.tree-view"; iconName: "phylogram"; enabled: window.speciesView !== null && window.current.display_path.length > 0; active: window.speciesView !== null && !window.speciesView.showGroups; onInvoked: window.speciesView.showGroups = false }
                        ModeButton { commandId: "species.text-view"; iconName: "text-representation"; enabled: window.speciesView !== null; active: window.speciesView !== null && window.speciesView.showGroups; onInvoked: window.speciesView.showGroups = true }
                        RibbonButton { commandId: "species.layout"; iconName: "bar-layout"; enabled: window.speciesView !== null && window.speciesView.columnInfo.columns.length > 0; onInvoked: window.speciesView.layoutBars() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    // The same tree style controls as the tree page, for the
                    // display tree; they change only how it is drawn here.
                    TreeStyleGroup {
                        scope: "species"
                        controller: window.speciesTree
                        customization: window.customization
                        onExtraRequested: treeStyle.dialog().openStyle("extra")
                        Component.onCompleted: commands.forEach(command => window.registerRibbonCommand(command))
                        RibbonButton { commandId: "species.support-style"; iconName: "support-style"; enabled: window.speciesTree !== null && window.speciesTree.has_support; onInvoked: treeStyle.dialog().openStyle("support") }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Hypotheses"
                        RibbonButton { commandId: "species.new"; iconName: "add"; enabled: window.speciesView !== null && !window.current.busy; onInvoked: window.speciesView.newHypothesis() }
                        RibbonButton { commandId: "species.columns"; iconName: "columns"; enabled: window.speciesView !== null; onInvoked: window.speciesView.chooseColumns() }
                        ModeButton { commandId: "species.details"; iconName: "info"; enabled: window.speciesView !== null; active: window.speciesView !== null && window.speciesView.detailsOpen; onInvoked: window.speciesView.detailsOpen = !window.speciesView.detailsOpen }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Tools"
                        ModeButton { commandId: "species.select-tool"; iconName: "selection"; enabled: window.speciesView !== null; active: window.speciesView !== null && window.speciesView.tool === "select"; onInvoked: window.speciesView.setTool("select") }
                        ModeButton { commandId: "species.threshold-cut"; tip: window.speciesView !== null && !window.current.can_cut ? "Needs an ultrametric display tree (all tips the same distance from the root)" : "Drag a threshold across the tree and save the species it defines"; iconName: "tree-threshold"; enabled: window.speciesView !== null && !window.current.busy && window.current.can_cut; active: window.speciesView !== null && window.speciesView.tool === "cut"; onInvoked: window.speciesView.setTool("cut") }
                        RibbonButton { commandId: "species.display"; tip: window.speciesView !== null && window.current.display_path.length ? "A display tree is attached" : "Attach or build a display tree"; iconName: "tree"; enabled: window.speciesView !== null && !window.current.busy && !window.current.display_path.length; onInvoked: window.beginDelimitation("view") }
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
                        RibbonButton { commandId: "trace.compute-calls"; enabled: window.current && !window.current.calling && window.traceDocument && window.traceDocument.traceRows.length === 1; onInvoked: window.current.compute_calls() }
                        RibbonButton { commandId: "trace.cancel-calls"; enabled: !!window.current && !!window.current.calling; onInvoked: window.current.cancel_calls() }
                    }
                    RibbonGroup {
                        title: "Issues"
                        disabledReason: !window.traceOpen || !window.current || !window.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
                        enabled: window.traceOpen && window.traceDocument && !window.traceDocument.working
                        RibbonButton { commandId: "ribbon.previous-unresolved-issue"; iconName: "previous"; keys: "Alt+← or ["; enabled: (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.issue(false) }
                        RibbonButton { commandId: "ribbon.next-unresolved-issue"; iconName: "next"; keys: "Alt+→ or ]"; enabled: (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.issue(true) }
                        RibbonButton { commandId: "ribbon.accept-this-base-and-go-to-the-next-issue"; iconName: "accept"; keys: "Alt+Enter"; enabled: (window.traceOpen && window.current && window.current.review_mode); onInvoked: window.traceDocument.accept() }
                        RibbonButton { commandId: "ribbon.choose-alignment-context"; enabled: (window.traceOpen && window.current && window.current.review_mode && window.traceDocument && window.traceDocument.returnAlignment === null); onInvoked: window.traceDocument.alignmentContext() }
                        RibbonButton { commandId: "ribbon.combine-up-to-three-reads"; enabled: (window.traceOpen && window.current && window.traceDocument && window.traceDocument.traceRows.length === 1); onInvoked: window.traceDocument.draftReads() }
                        RibbonButton { commandId: "ribbon.choose-the-terminal-signal-percentage-for-a-new-draft"; onInvoked: window.traceDocument.signalSettings() }
                    }
                    RibbonGroup {
                        title: "Masking"
                        disabledReason: !window.traceOpen || !window.current || !window.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
                        enabled: window.traceOpen && window.current && window.current.review_mode && window.traceDocument && !window.traceDocument.working
                        RibbonButton { commandId: "ribbon.choose-the-consensus-range-to-keep-in-exports"; onInvoked: window.traceDocument.maskEnds() }
                        RibbonButton { commandId: "ribbon.find-partial-terminal-primer-candidates-and-mask-them"; onInvoked: window.traceDocument.setPrimers() }
                        RibbonButton { commandId: "trace.keep-from"; iconName: "trace-mask"; onInvoked: window.traceDocument.setMaskStart(window.current.selected_column) }
                        RibbonButton { commandId: "trace.keep-through"; iconName: "trace-mask"; onInvoked: window.traceDocument.setMaskEnd(window.current.selected_column) }
                    }
                    RibbonGroup {
                        title: "Export consensus"
                        disabledReason: !window.traceOpen || !window.current || !window.current.review_mode ? "Open a Sanger review to use this group" : "Wait for trace processing to finish"
                        enabled: window.traceOpen && window.current && window.current.review_mode && window.traceDocument && !window.traceDocument.working
                        ExportConsensusButton { commandId: "ribbon.add-the-edited-consensus-to-its-alignment"; iconName: "consensus-to-alignment"; keys: "Ctrl+Shift+A"; enabled: window.traceDocument !== null && window.traceDocument.returnAlignment !== null && window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.addToAlignment() }
                        ExportConsensusButton { commandId: "ribbon.export-the-edited-consensus-as-fasta"; iconName: "fasta-document"; keys: "Ctrl+Shift+E"; enabled: window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.exportConsensus() }
                        ExportConsensusButton { commandId: "ribbon.copy-the-edited-consensus-as-fasta"; iconName: "clipboard"; keys: "Ctrl+Shift+C"; enabled: window.current && window.current.unresolved === 0; onInvoked: window.traceDocument.copyConsensus() }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }

                // Export: figures, specimen preparation and the complete GenBank workflow.
                RibbonPage {
                    spacing: 6 * window.ribbonSizeFactor
                    RibbonGroup {
                        title: "Figure"
                        AppRibbonButton { action: exportSvgCommand; visible: exportSvgCommand.visible; text: commandList.ribbon_label(exportSvgCommand.commandId); iconName: "save-as"; tip: exportSvgCommand.effectiveTooltip }
                        AppRibbonButton { action: exportPngCommand; visible: exportPngCommand.visible; text: commandList.ribbon_label(exportPngCommand.commandId); iconName: "figure-export"; tip: exportPngCommand.effectiveTooltip }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Zenodo"
                        RibbonButton { commandId: "export.zenodo"; iconName: "save-as"; tip: "Prepare selected saved files for a Zenodo draft"; enabled: shell.project_dir.length > 0; onInvoked: { const zenodo = zenodoWindow.item; if (zenodo && (zenodo.ready || zenodoJobs.busy || zenodo.attempted)) zenodo.open(); else zenodoPicker.dialog().open() } }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Specimens"
                        RibbonButton { commandId: "metadata.table"; iconName: "matrix"; tip: "The project's specimens, identifications and collection data"; enabled: shell.project_dir.length > 0; onInvoked: specimenTable.dialog().open() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Import"
                        RibbonButton { commandId: "metadata.import-headers"; iconName: "text-representation"; tip: window.metadataFasta.length ? "Read specimen IDs, taxa and values from this file's FASTA headers" : "Open a FASTA file first"; enabled: shell.project_dir.length > 0 && window.metadataFasta.length > 0; onInvoked: window.beginMetadataImport("headers", window.metadataFasta) }
                        RibbonButton { commandId: "metadata.import-sheet"; iconName: "add-file"; enabled: shell.project_dir.length > 0; onInvoked: metadataSheetPicker.dialog().open() }
                    }
                    ToolSeparator { Layout.fillHeight: true }
                    RibbonGroup {
                        title: "Records"
                        RibbonButton { commandId: "metadata.template"; iconName: "new-file"; tip: window.metadataFasta.length ? "Save an .ods sheet with a row per record of this file, to fill in" : "Open a FASTA file first"; enabled: shell.project_dir.length > 0 && window.metadataFasta.length > 0; onInvoked: metadataTemplateDestination.dialog().open() }
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
                        RibbonButton { commandId: "submission.new"; iconName: "new-file"; tip: window.submissionAlignment.length ? "Prepare a GenBank submission from this alignment" : "Open the marker alignment first"; enabled: shell.project_dir.length > 0 && window.submissionAlignment.length > 0; onInvoked: window.startSubmission() }
                        RibbonButton { commandId: "submission.open"; iconName: "open"; enabled: shell.project_dir.length > 0; onInvoked: submissionPicker.dialog().open() }
                        RibbonButton { commandId: "submission.prepare"; iconName: "edit"; tip: submissions.available ? "Rows, batch settings, reading frames, issues, package and accessions" : "Start or open a submission first"; enabled: submissions.available; onInvoked: window.showSubmission() }
                        RibbonButton { commandId: "submission.validate"; iconName: "accept"; enabled: submissions.available && !submissionJobs.busy; onInvoked: window.validateSubmission() }
                        RibbonButton { commandId: "submission.submitter"; iconName: "project"; onInvoked: window.showContacts() }
                    }
                    Item { Layout.fillWidth: true }
                    HelpGroup {}
                }
        }
    }

    // Settings for every plugin run, drawn from the plugin's manifest.
    OnDemand { id: toolSettings; objectName: "toolSettings"; sourceComponent: Component { ToolSettingsDialog { id: toolSettingsItem } } }
    OnDemand { id: operationWindow; sourceComponent: Component { OperationPickerDialog { id: operationPicker; onOpenResult: (path) => window.openPath(path) } } }

    OnDemand { id: shortcutsWindow; sourceComponent: Component { ShortcutsDialog { id: shortcuts; commands: window.applicationCommands } } }

    AppOperations { id: zenodoJobs; onBusyChanged: if (!busy && zenodoWindow.item) zenodoWindow.item.completed() }
    OnDemand { id: zenodoWindow; sourceComponent: Component { ZenodoDialog { id: zenodoDialog; backend: zenodoJobs; onChooseFiles: zenodoPicker.dialog().open() } } }
    OnDemand { id: zenodoPicker; sourceComponent: Component { FileDialog {
        id: zenodoPickerItem
        title: "Choose saved files for a Zenodo draft"
        fileMode: FileDialog.OpenFiles
        currentFolder: window.activeFolder()
        onAccepted: {
            let defaults = {}
            if (submissions.available) {
                const doc = JSON.parse(submissions.document || "{}")
                defaults = {title: (doc.publication || {}).title || "", creators: doc.publication_authors_text || ""}
            }
            zenodoWindow.dialog().show(Array.from(selectedFiles, url => shell.local_file_path(url)), shell.project_dir, defaults)
        }
    } } }

    // The project's specimen metadata (Export tab). It follows the
    // project folder; opening a project never creates metadata files.
    MetadataController { id: specimenMetadata }
    AutoSaveEvidence { controller: specimenMetadata; fileUrls: shell; evidenceService: provenance }
    readonly property string metadataProject: shell.project_dir
    onMetadataProjectChanged: {
        specimenMetadata.use_project(metadataProject)
        submissions.use_project(metadataProject)
    }
    readonly property var metadataTable: JSON.parse(specimenMetadata.table)
    // The open FASTA file whose records can be imported, linked or templated.
    readonly property string metadataFasta: current ? specimenMetadata.fasta_source(currentKind, current.file_path || "") : ""
    // Records of that file without a stored specimen link; only reported.
    readonly property var metadataLinks: metadataFasta.length && specimenMetadata.available && metadataTable
        ? JSON.parse(specimenMetadata.link_status(metadataFasta)) : ({})
    function metadataColumns() {
        const names = metadataTable.columns.map(c => c.name)
        for (const c of metadataTable.addable || []) names.push(c.name)
        return names
    }
    function beginMetadataImport(kind, path) {
        if (!specimenMetadata.available && !specimenMetadata.create()) { specimenTable.dialog().open(); return }
        const data = kind === "headers" ? JSON.parse(specimenMetadata.header_preview(path, "|"))
            : JSON.parse(specimenMetadata.sheet_preview(path))
        metadataImport.dialog().separator = "|"
        metadataImport.dialog().show(kind, path, data, metadataColumns(), metadataTable.patterns)
    }
    OnDemand { id: specimenTable; sourceComponent: Component { SpecimenTableDialog {
        id: specimenTableItem
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
    } } }

    OnDemand { id: metadataImport; sourceComponent: Component { MetadataImportDialog {
        id: metadataImportItem
        onPreviewRequested: separator => metadataImportItem.setPreview(JSON.parse(specimenMetadata.header_preview(metadataImportItem.file, separator)))
        onPlanRequested: (separator, roles, createSpecimens, overwrite) => {
            const ok = metadataImportItem.mode === "headers"
                ? specimenMetadata.plan_headers(metadataImportItem.file, separator, roles, createSpecimens, overwrite)
                : specimenMetadata.plan_sheet(metadataImportItem.file, roles, metadataImportItem.preview.has_document ? "" : window.metadataFasta, createSpecimens, overwrite)
            if (!ok) { metadataImportItem.message = specimenMetadata.message; return }
            metadataImportItem.close()
            metadataReview.dialog().show(JSON.parse(specimenMetadata.plan), metadataImportItem.mode === "headers" ? "pattern" : "mapping")
        }
    } } }
    // GenBank marker submissions (Export tab). table2asn runs as a
    // registered operation on its own job runner, like BLAST.
    SubmissionController { id: submissions; onOpened: success => { if (success) window.showSubmission() } }
    AutoSaveEvidence { controller: submissions; fileUrls: shell; evidenceService: provenance }
    Connections {
        target: specimenMetadata
        function onTableChanged() { if (submissions.available) submissions.refresh_review() }
    }
    // The open alignment a submission can start from.
    readonly property string submissionAlignment: alignmentOpen ? metadataFasta : ""
    AppOperations {
        id: submissionJobs
        onBusyChanged: if (!busy) submissions.record_validation(outcome)
    }
    function startSubmission() {
        const file = submissionAlignment.split("/").pop()
        submissionName.dialog().ask("Start a GenBank submission", "Submission name", file.replace(/\.[^.]*$/, ""), name => {
            if (!submissions.create(submissionAlignment, name)) return submissions.message
            showSubmission()
            return ""
        })
    }
    function showSubmission() {
        submissions.refresh_review()
        submissionWindow.dialog().open()
    }
    function validateSubmission() {
        if (!submissions.export_package()) { showSubmission(); return }
        const request = submissions.validation_request()
        if (request.length) submissionJobs.submit(shell.project_dir, request)
        if (!submissionWindow.item || !submissionWindow.item.opened) showSubmission()
    }
    OnDemand { id: submissionName; sourceComponent: Component { TextPromptDialog { id: submissionNameItem } } }
    OnDemand {
        id: submissionWindow
        sourceComponent: Component {
            SubmissionDialog {
                id: submissionDialog
                onContactDetailsRequested: window.showContacts()
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
                onAccessionTableRequested: accessionTableDestination.dialog().open()
            }
        }
    }
    OnDemand { id: firstLaunch; objectName: "firstLaunch"; sourceComponent: Component { PreferencesDialog { welcome: true; settings: preferences; commands: window.applicationCommands; defaultFontSizePoints: window.defaultFontSizePoints } } }
    // One contact-details dialog: the GenBank submitter, then other services.
    function showContacts() {
        const prefs = JSON.parse(preferences.snapshot || "{}")
        submitterDialog.dialog().show(prefs.submitter, (prefs.contacts || {}).service_emails)
    }
    OnDemand { id: submitterDialog; sourceComponent: Component { SubmitterDialog {
        id: submitterDialogItem
        onSaved: (value, services) => {
            preferences.update("submitter", JSON.stringify(value))
            preferences.update("serviceEmail", JSON.stringify({service: "blast", email: services.blast}))
            if (submissions.available) submissions.refresh_review()
        }
    } } }

    OnDemand { id: submissionPicker; sourceComponent: Component { FileDialog {
        id: submissionPickerItem
        title: "Open a GenBank submission"
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Submissions (*.submission.json)"]
        onAccepted: submissions.open(shell.local_file_path(selectedFile))
    } } }

    OnDemand { id: accessionTableDestination; sourceComponent: Component { FileDialog {
        id: accessionTableDestinationItem
        title: "Save the accession table"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "tsv"
        nameFilters: ["Tab-separated table (*.tsv)"]
        onAccepted: submissions.write_accession_table(shell.local_file_path(selectedFile))
    } } }

    OnDemand { id: metadataReview; sourceComponent: Component { MetadataReviewDialog {
        id: metadataReviewItem
        onApplyRequested: remember => specimenMetadata.apply_plan(remember)
        onDropped: specimenMetadata.clear_plan()
    } } }

    OnDemand { id: metadataSheetPicker; sourceComponent: Component { FileDialog {
        id: metadataSheetPickerItem
        title: "Import specimen data from a spreadsheet"
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Spreadsheets (*.ods *.xlsx *.xls *.csv *.tsv)", "All files (*)"]
        onAccepted: window.beginMetadataImport("sheet", shell.local_file_path(selectedFile))
    } } }

    OnDemand { id: metadataTemplateDestination; sourceComponent: Component { FileDialog {
        id: metadataTemplateDestinationItem
        title: "Save the specimen data template"
        fileMode: FileDialog.SaveFile
        defaultSuffix: "ods"
        nameFilters: ["OpenDocument spreadsheet (*.ods)"]
        onAccepted: {
            if (!specimenMetadata.available) specimenMetadata.create()
            specimenMetadata.write_template(window.metadataFasta, shell.local_file_path(selectedFile))
        }
    } } }

    OnDemand { id: orfDialog; sourceComponent: Component { AppContentDialog {
        id: orfDialogItem
        initialFocusItem: codons
        property bool inSelection: false
        property var source: null
        property string sourcePath: ""
        property var codes: []
        property int suggested: -1
        property string suggestedFrom: ""
        function search(selectionOnly) {
            if (!window.current || window.current.busy) return
            source = window.current
            sourcePath = source.file_path
            inSelection = selectionOnly
            codes = JSON.parse(source.genetic_codes())
            const code = source.genetic_code()
            suggested = codes.findIndex(c => c.id === code)
            suggestedFrom = source.genetic_code_source()
            orfCode.currentIndex = suggested
            open()
        }
        title: inSelection ? "Find open reading frames in the selection" : "Find open reading frames"
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: codons.forceActiveFocus()
        onAccepted: {
            window.orfMinCodons = codons.value
            if (orfCode.currentIndex < 0) return
            if (source && source.file_path === sourcePath && !source.busy) source.find_orfs(codons.value, codes[orfCode.currentIndex].id, orfDialogItem.inSelection)
        }
        body: ColumnLayout {
            spacing: 8
            RowLayout {
                Label { id: orfCodeLabel; text: "Genetic code" }
                AppComboBox {
                    id: orfCode; objectName: "orfCode"; Accessible.labelledBy: orfCodeLabel; Accessible.name: orfCodeLabel.text
                    Layout.fillWidth: true
                    model: orfDialogItem.codes
                    textRole: "name"
                }
                Label {
                    text: orfCode.currentIndex === orfDialogItem.suggested ? "(" + orfDialogItem.suggestedFrom + ")" : ""
                    opacity: 0.7
                }
            }
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
                    Keys.onReturnPressed: orfDialogItem.accept()
                    Keys.onEnterPressed: orfDialogItem.accept()
                }
                Label { text: "codons, start to stop" }
            }
            Label {
                text: "Searches both strands. Each ORF is added as a misc_feature with the note \"ORF\";\none Undo removes them all."
                opacity: 0.7
            }
        }
    } } }

    OnDemand { id: genesDialog; sourceComponent: Component { AppContentDialog {
        id: genesDialogItem
        initialFocusItem: codeBox
        property bool inSelection: false
        property var source: null
        property string sourcePath: ""
        function predict(selectionOnly) {
            if (!window.current || window.current.busy) return
            source = window.current
            sourcePath = source.file_path
            inSelection = selectionOnly
            // Where the record states no code, Prodigal's usual inputs
            // (bacteria, archaea, plastids) use table 11, not the standard
            // code: its start rule differs.
            const stated = source.genetic_code_source() !== "default"
            const code = stated ? source.genetic_code() : 11
            let i = -1
            for (let k = 0; k < geneticCodes.count; k++)
                if (geneticCodes.get(k).code === code) i = k
            codeBox.currentIndex = i
            suggested = i
            suggestedFrom = stated ? source.genetic_code_source() : "usual for bacteria, archaea and plastids"
            open()
        }
        property int suggested: -1
        property string suggestedFrom: ""
        title: inSelection ? "Predict genes in the selection" : "Predict genes"
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: codeBox.forceActiveFocus()
        onAccepted: if (codeBox.currentIndex >= 0 && source && source.file_path === sourcePath && !source.busy) source.predict_genes(geneticCodes.get(codeBox.currentIndex).code, inSelection)
        body: ColumnLayout {
            spacing: 8
            RowLayout {
                Label { id: codeBoxLabel; text: "Genetic code" }
                AppComboBox {
                    id: codeBox; objectName: "codeBox"; Accessible.labelledBy: codeBoxLabel; Accessible.name: codeBoxLabel.text
                    Layout.fillWidth: true
                    model: geneticCodes
                    textRole: "label"
                    Keys.onReturnPressed: genesDialogItem.accept()
                    Keys.onEnterPressed: genesDialogItem.accept()
                }
                Label {
                    text: codeBox.currentIndex === genesDialogItem.suggested ? "(" + genesDialogItem.suggestedFrom + ")" : ""
                    opacity: 0.7
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
    } } }

    OnDemand { id: about; sourceComponent: Component { AboutDialog {
        id: aboutItem
        version: shell.version()
        backgroundColor: shell.background
        onShortcutsRequested: shortcutsCommand.trigger()
    } } }

    OnDemand { id: openDialog; sourceComponent: Component { FileDialog {
        id: openDialogItem
        title: "Open a sequence, alignment, trace, review or tree"
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Sequences, alignments, traces and trees (*.gb *.gbk *.genbank *.gbff *.fasta *.fa *.fna *.fas *.ffn *.faa *.afa *.ab1 *.abi *.json *.nwk *.newick *.tre *.tree *.trees *.treefile *.contree *.nex *.nexus *.nxs)", "All files (*)"]
        onAccepted: window.openPath(shell.local_file_path(selectedFile))
    } } }

    function saveTarget(owner, path, continuation) {
        evidenceSave.request = {owner: owner, continuation: continuation}
        evidenceSave.owner = null
        evidenceSave.target = path || ""
        const step = owner ? documentOpens.save_step(documentOpens.target_index(owner.token, owner.path), false) : ""
        if (!path && step === "destination") evidenceSave.chooseDestination()
        else evidenceSave.inspect(true)
    }
    AutoSaveEvidence {
        id: evidenceSave
        property var request: null
        controller: request && request.owner ? request.owner.controller : null
        evidenceService: provenance
        fileUrls: shell
        automatic: false
        dialogName: "evidenceSaveDialog"
        valid: () => request && documentOpens.target_index(request.owner.token, request.owner.path) >= 0
        chooseDestination: () => window.saveAs(request.owner, request.continuation)
        onFailed: message => provenance.notification(target || request.owner.path, message, "fail")
        onFinished: success => {
            const owner = request.owner
            if (success && target) {
                documentOpens.path_changed(owner.token, target, owner.controller.file_path)
                owner.path = target
                Qt.callLater(window.refreshCurrent)
            }
            request.continuation && request.continuation(success)
        }
    }
    // Save the active tab under a new name; the tab then shows the new file.
    // D13: copy the file whose folder refused the edit to where the user chooses,
    // open the copy in place of the unedited original and redo the edit there.
    function saveCopy(refused) {
        const row = refused.row !== undefined ? refused.row : tabs.currentIndex
        saveDialog.dialog().target = null
        saveDialog.dialog().continuation = path => {
            const error = documentOpens.copy_for_editing(JSON.stringify(refused), path)
            if (error.length) { provenance.notification(refused.file, error, "fail"); return }
            removeDocument(row)
            openPath(path)
        }
        saveDialog.dialog().selectedFile = shell.local_file_url(refused.file)
        saveDialog.dialog().open()
    }
    function saveAs(owner = documentTarget(tabs.currentIndex), continuation = null) {
        if (!owner) return
        const index = documentOpens.target_index(owner.token, owner.path)
        if (documentOpens.save_step(index, true) === "trace") { window.documentAt(index).saveReview(); return }
        saveDialog.dialog().target = owner
        saveDialog.dialog().continuation = continuation
        saveDialog.dialog().selectedFile = shell.local_file_url(owner.path.startsWith("untitled:") ? window.fileName(owner.path) : owner.path)
        saveDialog.dialog().open()
    }

    Shortcut { sequence: "Ctrl+Q"; onActivated: window.close() }
    // Close the active tab, asking first if it has unsaved changes.
    Shortcut { sequence: "Ctrl+W"; onActivated: if (documentOpens.count > 0) window.closeTab(tabs.currentIndex) }
    // Undo and redo reach the open document wherever the focus is; a text
    // field being edited keeps its own. Trace review has its own undo.
    Shortcut { sequence: "Ctrl+Z"; enabled: !(submissionWindow.item && submissionWindow.item.opened) && window.current !== null && !window.traceOpen; onActivated: window.current.undo() }
    Shortcut { sequences: ["Ctrl+Shift+Z", "Ctrl+Y"]; enabled: !(submissionWindow.item && submissionWindow.item.opened) && window.current !== null && !window.traceOpen; onActivated: window.current.redo() }
    OnDemand { id: saveDialog; sourceComponent: Component { FileDialog {
        id: saveDialogItem
        objectName: "saveDocumentDialog"
        onRejected: { target = null; continuation = null }
        property var target: null
        property var continuation: null
        title: "Save as"
        fileMode: FileDialog.SaveFile
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        onAccepted: {
            const owner = target
            const path = shell.local_file_path(selectedFile)
            const next = continuation
            target = null
            continuation = null
            if (owner) window.saveTarget(owner, path, next); else next(path)
        }
    } } }

    Component {
        id: projectDialogComponent
        FolderDialog {
            parentWindow: window
            title: "Open or create a project folder"
            onAccepted: {
                window.selectProject(shell.local_file_path(selectedFolder))
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
                objectName: "documentTabs"
                Layout.fillWidth: true
                visible: documentOpens.count > 0
                onCurrentIndexChanged: { if (!window.applyingOpenFocus) documentOpens.revoke_focus(); Qt.callLater(window.refreshCurrent) }
                Repeater {
                    model: documentOpens
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
                visible: documentOpens.count > 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: tabs.currentIndex

                Repeater {
                    id: docs
                    model: documentOpens
                    onItemAdded: Qt.callLater(window.refreshCurrent)
                    onItemRemoved: Qt.callLater(window.refreshCurrent)
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
                                        const row = documentOpens.index_of(token)
                                        if (row >= 0) {
                                            provenance.notification(documentOpens.path_at(row), "Could not construct the document view", "fail")
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
                                    function onDirtyChanged() { documentOpens.set_dirty(canvasSlot.index, canvasLoader.item.dirty) }
                                    function onActivate() { tabs.currentIndex = canvasSlot.index }
                                    function onOpenFile(path) { window.openPath(path) }
                                    function onReviewTraces(paths, reference) { window.startTraceReview(paths, reference, canvasLoader.item) }
                                    function onToggleInsert() { window.alignmentInsert = !window.alignmentInsert }
                                    function onContextMenu(info) {
                                        if (canvasLoader.item.kind === "tree") treeMenu.dialog().showFor(canvasSlot.admittedController, info)
                                        else if (canvasLoader.item.kind === "alignment") { alnMenu.info = info; alnMenu.popup() }
                                        else { menu.info = info; menu.popup() }
                                    }
                                    function onCopyText(text) { clipboard.put(text) }
                                    function onToggleSidebar() { window.sidebarOpen = !window.sidebarOpen }
                                    function onRenameRow() { renameDialog.dialog().askRow() }
                                    function onRenameTip() { tipDialog.dialog().ask() }
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
                                onReadOnlyFolder: path => readOnlyCopy.dialog().offered = {file: path, review: true, row: index}
                                onDirtyChanged: documentOpens.set_dirty(index, dirty)
                                onWorkingChanged: documentOpens.set_working(index, working)
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
                                onDirtyChanged: documentOpens.set_dirty(index, dirty)
                                onContextMenu: (info) => { alnMenu.info = info; alnMenu.popup() }
                                onCopyText: (text) => clipboard.put(text)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameRow: renameDialog.dialog().askRow()
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
                                onDirtyChanged: documentOpens.set_dirty(index, dirty)
                                onContextMenu: (info) => treeMenu.dialog().showFor(treeDocument.controller, info)
                                onToggleSidebar: window.sidebarOpen = !window.sidebarOpen
                                onRenameTip: tipDialog.dialog().ask()
                                onAnnotate: cladeEditor.openSelected()
                            }
                        }
                        DelegateChoice {
                            roleValue: "delimitation"
                            DelimitationView {
                                overviewEnabled: window.overviewEnabled
                                notificationSink: provenance
                                onOpenFile: (path) => window.openPath(path)
                                onSmoothingRequested: path => operationWindow.dialog().showOperation(shell.project_dir.length ? shell.project_dir : shell.local_file_path(shell.parent_folder_url(path)), path, false, "basecraft.sanderson/date_tree@1")
                                onDirtyChanged: documentOpens.set_dirty(index, dirty)
                            }
                        }
                        DelegateChoice {
                            DocumentView {
                                id: sequenceDocument
                                overviewEnabled: window.overviewEnabled
                                notificationSink: provenance
                                showGc: window.showGc
                                defaultLayout: window.defaultLayout
                                onDirtyChanged: documentOpens.set_dirty(index, dirty)
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
                visible: documentOpens.count === 0 && shell.project_dir.length === 0
                Layout.fillWidth: true
                Layout.fillHeight: true
                backgroundColor: shell.background
                recentProjects: preferences.recent_projects
                onOpenProject: window.openProjectPicker()
                onOpenRecent: path => window.selectProject(path)
            }

            Item {
                visible: documentOpens.count === 0 && shell.project_dir.length > 0
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
                visible: documentOpens.count > 0
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

    AppOperations { id: blastBackend; onBusyChanged: if (!busy && blastWindow.item) blastWindow.item.completed() }
    OnDemand { id: blastWindow; sourceComponent: Component { BlastDialog {
        id: blastDialog; backend: blastBackend; onCopyText: text => clipboard.put(text)
        onSaveContact: email => preferences.update("serviceEmail", JSON.stringify({service: "blast", email: email}))
        onContactDetailsRequested: window.showContacts()
    } } }
    function beginBlast(mode, feature) {
        const controller = window.current
        mode = commandList.blast_source(window.alignmentOpen, mode, !window.alignmentOpen && controller.has_selection())
        const annotation = window.alignmentOpen ? -1 : (feature === undefined ? controller.selected_feature() : feature)
        blastWindow.dialog().show(source => controller.blast_queries(source, annotation), mode, window.alignmentOpen, controller.record_folder())
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
        Item { text: "Find ORFs in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection && window.current && !window.current.busy; onTriggered: orfDialog.dialog().search(true) }
        Item { text: "Predict genes in selection…"; visible: !menu.onAnnotation; enabled: menu.info.selection && window.current && !window.current.busy; onTriggered: genesDialog.dialog().predict(true) }
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
        AlnItem { text: "Rename “" + alnMenu.info.rowName + "”…"; visible: alnMenu.info.rows === 1 && !alnMenu.columns; onTriggered: renameDialog.dialog().askRow() }
        AlnItem { text: "Move to top"; visible: alnMenu.rows; onTriggered: window.current.rows_to_top() }
        AlnItem { text: alnMenu.info.rows === 1 ? "Delete row" : "Delete " + alnMenu.info.rows + " rows"; visible: alnMenu.rows; onTriggered: window.current.remove_rows() }
        AlnItem { text: "Insert gap column"; visible: alnMenu.columns; onTriggered: window.current.insert_column() }
        AlnItem { text: alnMenu.info.columns === 1 ? "Delete column" : "Delete " + alnMenu.info.columns + " columns"; visible: alnMenu.columns; onTriggered: window.current.delete_columns() }
        AlnItem { text: "Strip gap-only columns"; visible: alnMenu.columns || alnMenu.rows; onTriggered: window.current.remove_gap_columns() }
        AlnItem { text: "BLAST selected rows at NCBI"; onTriggered: window.beginBlast("rows") }
        AlnItem { text: "Copy as FASTA"; onTriggered: clipboard.put(window.current.selection_text()) }
    }

    OnDemand { id: renameDialog; sourceComponent: Component { TextPromptDialog {
        id: renameDialogItem
        function askRow() {
            if (window.alignmentOpen)
                ask("Rename row", "Row name", window.current.row_name(), value => window.current.rename_row(value))
        }
    } } }

    // Tree context menu: what applies to the clade or tip under the pointer.
    OnDemand { id: treeMenu; sourceComponent: Component { TreeContextMenu {
        id: treeMenuItem
        onNameRequested: (target, annotation, revision) => cladeEditor.openCaptured(target, annotation, revision, true)
        onRenameTipRequested: (target, name, revision) => tipDialog.dialog().openCaptured(target, name, revision)
        onRootPositionRequested: rootDialog.dialog().ask()
        onSupportRequested: supportFile.dialog().open()
        onStyleRequested: treeStyle.dialog().openStyle("extra")
    } } }

    // Where the root sits along its branch: a share of the branch on each
    // side. Halfway after rerooting.
    OnDemand { id: rootDialog; sourceComponent: Component { AppContentDialog {
        id: rootDialogItem
        initialFocusItem: rootShare
        property var info: ({ side: "", other: "", length: 0, share: 0.5 })
        title: "Root position"
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
                Label { text: rootDialogItem.info.side; elide: Text.ElideMiddle; Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true; horizontalAlignment: Text.AlignRight }
                Slider { id: rootShare; Accessible.labelledBy: rootShareLabel; Accessible.name: rootShareLabel.text + " (%)"; from: 0; to: 100; stepSize: 0.5; Layout.preferredWidth: 220; Layout.minimumWidth: 0; Layout.fillWidth: true }
                Label { text: rootDialogItem.info.other; elide: Text.ElideMiddle; Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true }
            }
            GridLayout {
                columns: width < 600 ? 1 : 3
                Layout.fillWidth: true
                Label {
                    text: rootShare.value.toFixed(1) + " % · " + rootDialogItem.length(rootShare.value / 100)
                    Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true
                    horizontalAlignment: Text.AlignRight
                }
                AppButton { text: "Halfway"; flat: true; Layout.preferredWidth: 220; Layout.minimumWidth: 0; Layout.fillWidth: true; onClicked: rootShare.value = 50 }
                Label { text: (100 - rootShare.value).toFixed(1) + " % · " + rootDialogItem.length(1 - rootShare.value / 100); Layout.preferredWidth: 200; Layout.minimumWidth: 0; Layout.fillWidth: true }
            }
        }
    } } }

    OnDemand { id: metadataTipLabels; sourceComponent: Component { SpecimenTreeLabelsDialog {
        id: metadataTipLabelsItem
        metadataController: specimenMetadata
        table: window.metadataTable
        projectDir: shell.project_dir
    } } }

    OnDemand { id: rosterConsumer; sourceComponent: Component { SpecimenRosterDialog {
        id: rosterConsumerItem
        metadataController: specimenMetadata
        table: window.metadataTable
        projectDir: shell.project_dir
    } } }

    OnDemand { id: tipDialog; sourceComponent: Component { TreeTipNameDialog {
        id: tipDialogItem
        controller: window.treeOpen ? window.current : null
    } } }

    // Support from another tree: pick the file, then name the values.
    OnDemand { id: supportFile; sourceComponent: Component { FileDialog {
        id: supportFileItem
        title: "Add support values from another tree"
        onVisibleChanged: if (visible) currentFolder = window.activeFolder()
        nameFilters: ["Trees (*.nwk *.newick *.tre *.tree *.trees *.treefile *.contree *.nex *.nexus *.nxs)", "All files (*)"]
        onAccepted: {
            supportName.dialog().controller = window.current
            supportName.dialog().path = shell.local_file_path(selectedFile)
            supportName.dialog().metadata = JSON.parse(supportName.dialog().controller.support_import_json(supportName.dialog().path))
            supportName.dialog().open()
        }
    } } }

    OnDemand { id: supportName; sourceComponent: Component { AppContentDialog {
        id: supportNameItem
        initialFocusItem: supportNameField
        property string path
        property var controller: null
        property var metadata: ({sets: []})
        title: "Name the support values"
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: { supportNameField.text = metadata.sets.length === 1 ? metadata.sets[0].name : ""; supportNameField.forceActiveFocus() }
        onAccepted: if (controller && !metadata.error) controller.add_support_from(path, supportNameField.text)
        body: ColumnLayout {
            spacing: 6
            Label { text: window.fileName(supportNameItem.path) }
            Label { text: supportNameItem.metadata.error || supportNameItem.metadata.sets.map(set => set.name).join(" / "); wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label { id: supportNameFieldLabel; text: supportNameItem.metadata.sets.length > 1 ? "Optional prefix (individual names are retained)" : "Support values name" }
            TextField {
                Accessible.labelledBy: supportNameFieldLabel
                Accessible.name: supportNameFieldLabel.text
                id: supportNameField
                Layout.preferredWidth: 320
                placeholderText: "e.g. Bootstrap, PP, UFBoot; empty keeps the file's names"
                Keys.onReturnPressed: supportNameItem.accept()
                Keys.onEnterPressed: supportNameItem.accept()
            }
        }
    } } }

    CladeEditor {
        id: cladeEditor
        controller: window.treeOpen ? window.current : null
    }
    OnDemand { id: treeStyle; sourceComponent: Component { TreeStyleDialog {
        id: treeStyleItem
        controller: window.treeOpen ? window.current : window.speciesTree
    } } }

    OnDemand { id: linkAlignmentFile; sourceComponent: Component { FileDialog {
        id: linkAlignmentFileItem
        property var editController: null
        property string editRevision: ""
        function ask(target) {
            editController = target
            editRevision = target.record_revision()
            open()
        }
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
            editController.link_alignment(shell.local_file_path(selectedFile))
        }
    } } }

    function exportSvg(format) {
        svgOptions.dialog().format = format || "svg"
        if (!window.current) return
        svgDialog.dialog().controller = window.current
        svgDialog.dialog().sourcePath = window.currentPath
        svgOptions.dialog().alignment = window.alignmentOpen
        svgOptions.dialog().open()
    }
    OnDemand { id: svgOptions; sourceComponent: Component { AppContentDialog {
        id: svgOptionsItem
        onAboutToShow: {
            svgScope.currentIndex = window.treeOpen ? 2 : 0
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
            svgDialog.dialog().format = svgOptionsItem.format
            svgDialog.dialog().scope = svgScope.currentIndex
            svgDialog.dialog().publication = svgPublication.checked
            svgDialog.dialog().alignment = svgOptionsItem.alignment
            svgDialog.dialog().includeSelection = svgSelection.checked
            svgDialog.dialog().rasterWidth = pngWidth.value
            svgDialog.dialog().selectedFile = shell.local_file_url(svgDialog.dialog().sourcePath.replace(/\.[^./]*$/, "") + "." + svgDialog.dialog().format)
            svgDialog.dialog().open()
        }
    } } }

    OnDemand { id: svgDialog; sourceComponent: Component { FileDialog {
        id: svgDialogItem
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
            if (alignment && format === "png") controller.export_figure_png_selection(shell.local_file_path(selectedFile), scope, publication, rasterWidth, includeSelection)
            else if (alignment) controller.export_figure_selection(shell.local_file_path(selectedFile), scope, publication, includeSelection)
            else if (format === "png") controller.export_figure_png(shell.local_file_path(selectedFile), scope, publication, rasterWidth)
            else controller.export_figure(shell.local_file_path(selectedFile), scope, publication)
        }
    } } }

    // Quitting with unsaved changes: save them all, throw them away, or
    // stay.
    OnDemand { id: quitDialog; sourceComponent: Component { AppFormDialog {
        id: quitDialogItem
        objectName: "quitDocumentsDialog"
        property var unsaved: []
        title: "Save changes before closing?"
        standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
        function ask(targets, error) {
            unsaved = targets
            quitError.text = error || ""
            open()
        }
        ColumnLayout {
            Kirigami.FormData.isSection: true
            Layout.fillWidth: true
            spacing: 6
            Label { Layout.fillWidth: true; wrapMode: Text.Wrap; text: quitDialogItem.unsaved.length === 1 ? "This file has unsaved changes:" : "These files have unsaved changes:" }
            Repeater {
                model: quitDialogItem.unsaved
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
        onAccepted: window.quitStep(JSON.parse(documentOpens.save_all(JSON.stringify(unsaved))))
        onDiscarded: { close(); window.quitStep(JSON.parse(documentOpens.discard_all(JSON.stringify(unsaved)))) }
    } } }

    OnDemand { id: confirmClose; sourceComponent: Component { AppFormDialog {
        id: confirmCloseItem
        objectName: "closeDocumentDialog"
        property var target: null
        property string saveError: ""
        title: "Save changes?"
        standardButtons: Dialog.Save | Dialog.Discard | Dialog.Cancel
        AppErrorLabel {
            Accessible.role: confirmCloseItem.saveError.length ? Accessible.AlertMessage : Accessible.StaticText
            Kirigami.FormData.isSection: true
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: confirmCloseItem.saveError.length ? confirmCloseItem.saveError : "This file has unsaved changes."
        }
        onAccepted: {
            const owner = target
            if (documentOpens.target_index(owner.token, owner.path) < 0) return
            window.saveTarget(owner, "", function(success) {
                const row = documentOpens.target_index(owner.token, owner.path)
                if (row < 0) return
                if (!success) {
                    confirmCloseItem.target = owner
                    confirmCloseItem.saveError = owner.controller.message || "Save the review to a file first"
                    confirmCloseItem.open()
                    return
                }
                confirmCloseItem.saveError = ""
                window.removeDocument(row)
                Qt.callLater(window.refreshCurrent)
            })
        }
        onDiscarded: {
            const row = documentOpens.target_index(target.token, target.path)
            close()
            if (row < 0) return
            window.removeDocument(row)
            Qt.callLater(window.refreshCurrent)
        }
    } } }

    FeatureEditor {
        id: editor
        // Annotations belong to sequences; alignments have none yet.
        controller: window.currentKind === "sequence" ? window.current : null
    }

    Component.onCompleted: {
        loadPreferences()
        if (preferenceValues.ui && !preferenceValues.ui.welcomed && !scriptedRun) firstLaunch.dialog().open()
        preferences.check_release()
        computeReady = true
        localCpuThreads = Math.max(1, Math.min(availableCpuThreads, localCpuThreads))
        shell.configure_cpu_threads(localCpuThreads)
        const args = Qt.application.arguments
        for (let i = 1; i < args.length; i++) {
            if (args[i].startsWith("--")) { i++; continue }
            openPath(args[i])
        }
    }
}
