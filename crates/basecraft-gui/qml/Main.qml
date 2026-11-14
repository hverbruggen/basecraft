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
    readonly property GenomeController shell: GenomeController { objectName: "shell" }

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
        const slot = documentArea.docs.itemAt(index)
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
    readonly property DocumentOpens documentOpens: DocumentOpens {
        onAcquired: (token, kind) => {
            const index = documentOpens.index_of(token)
            const slot = index >= 0 ? documentArea.docs.itemAt(index) : null
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
    onCurrentChanged: { alphabetQuestion.dialog().controller = current; duplicateNames.dialog().controller = current }
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
    readonly property var applicationCommands: [findCommand, newSequenceCommand, addAnnotationCommand, openCommand, saveCommand, saveAsCommand, projectCommand, shortcutsCommand, aboutCommand, preferencesCommand, textRepresentationCommand, contextShortcutsCommand, learnCommand, exportSvgCommand, exportPngCommand, historyCommand, methodsCommand, showProvenanceCommand, verifyProvenanceCommand, recoverCommand, installedOperationsCommand].concat(ribbonCommands)
    // Another project closes the open documents first, asking as quitting does.
    // A task still running in the old project, from a tab or not, refuses it first.
    function selectProject(path, then = "") {
        const chosen = preferences.select_project(path)
        if (chosen.length) quitStep(JSON.parse(documentOpens.switch_project(chosen, shell.project_dir, then)))
    }
    property bool preferencesReady: false
    readonly property AppSettings preferences: AppSettings {
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
    OnDemand { id: preferencesError; objectName: "preferencesError"; sourceComponent: Component { PreferencesErrorDialog { app: window } } }
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
    OnDemand { id: duplicateNames; objectName: "duplicateNames"; sourceComponent: Component { DuplicateNamesDialog {} } }
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
        const regions = [ribbonTabBar, documentArea.fileTree, tabs, delimitationOpen ? window.documentAt(tabs.currentIndex) : currentView].filter(item => item && item.visible && item.enabled)
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
        function onTaxaSelected(names) {
            for (let i = 0; i < documentArea.docs.count; i++) {
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
    OnDemand { id: newSequenceDialog; sourceComponent: Component { NewSequenceDialog { app: window } } }
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
    readonly property AppProvenance provenance: AppProvenance {
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
        id: recoverCommand
        commandId: "file.recover-work"
        iconName: "open"
        enabled: !provenance.busy
        onTriggered: {
            recoveryPicker.dialog().sessions = JSON.parse(provenance.recovery_sessions())
            recoveryPicker.dialog().open()
        }
    }
    OnDemand { id: recoveryPicker; sourceComponent: Component { RecoveryPickerDialog { app: window; destinationFolder: provenanceFolder } } }

    OnDemand { id: provenanceFolder; sourceComponent: Component { FolderDialog {
        id: provenanceFolderItem
        title: "Choose the parent folder for recovered copies"
        onAccepted: {
            const destination = shell.local_file_path(selectedFolder) + "/" + window.fileName(window.provenanceSource) + ".recovered"
            provenanceDialog.dialog().open()
            provenance.start(window.provenanceExport, window.provenanceSource, destination)
        }
    } } }

    OnDemand { id: provenanceDialog; sourceComponent: Component { ProvenanceDialog { app: window } } }
    OnDemand { id: preferencesWindow; sourceComponent: Component { PreferencesDialog { id: preferencesDialog; settings: preferences; commands: window.applicationCommands; defaultFontSizePoints: window.defaultFontSizePoints; onContactDetailsRequested: window.showContacts() } } }

    // The one list of commands: keys, scopes, icons and why one is unavailable.
    readonly property AppCommands commandList: AppCommands {}
    function commandFor(id) { return applicationCommands.find(command => command.commandId === id) || null }
    readonly property string commandContext: JSON.stringify({project: shell.project_dir.length > 0, document: current !== null,
        path: currentPath.length > 0, kind: currentKind, busy: !!current && !!current.busy, calling: !!current && !!current.calling,
        linked_alignment: !!current && !!current.linked_alignment, species_view: speciesView !== null, records_busy: provenance.busy,
        submission_busy: submissionJobs.busy, trace_working: traceOpen && !!traceDocument && traceDocument.working,
        review_mode: !!current && !!current.review_mode, unresolved: !!current && current.unresolved > 0})
    component Command: AppCommand {
        text: window.commandList.caption(commandId)
        tooltip: window.commandList.tip(commandId)
        customization: window.customization
        disabledReason: window.commandList.disabled_reason(commandId, window.commandContext)
        hintScope: window.commandList.scope(commandId)
        defaultKeyHint: window.commandList.key(commandId)
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

            RibbonFilePage {
                app: window; combinePicker: combinePicker; projectCommand: projectCommand
                newSequenceCommand: newSequenceCommand; saveCommand: saveCommand; saveAsCommand: saveAsCommand
                historyCommand: historyCommand; methodsCommand: methodsCommand; recoverCommand: recoverCommand
            }
            RibbonViewPage { app: window; findCommand: findCommand; textRepresentationCommand: textRepresentationCommand }
            RibbonSequencePage {
                app: window; addAnnotationCommand: addAnnotationCommand; editor: editor; clipboard: clipboard
                orfDialog: orfDialog; genesDialog: genesDialog
            }
            RibbonAlignmentPage { app: window; renameDialog: renameDialog }
            RibbonTreePage {
                app: window; operationWindow: operationWindow; cladeEditor: cladeEditor; treeStyle: treeStyle
                metadataTipLabels: metadataTipLabels; supportFile: supportFile; linkAlignmentFile: linkAlignmentFile
            }
            RibbonPhylogenyPage { app: window; toolSettings: toolSettings }
            RibbonSpeciesPage {
                app: window; specimenMetadata: specimenMetadata; rosterConsumer: rosterConsumer; treeStyle: treeStyle
                combinePicker: combinePicker
            }
            RibbonTracePage { app: window }
            RibbonExportPage {
                app: window; exportSvgCommand: exportSvgCommand; exportPngCommand: exportPngCommand
                zenodoWindow: zenodoWindow; zenodoJobs: zenodoJobs; zenodoPicker: zenodoPicker; specimenTable: specimenTable
                metadataSheetPicker: metadataSheetPicker; metadataTemplateDestination: metadataTemplateDestination
                submissions: submissions; submissionJobs: submissionJobs; submissionPicker: submissionPicker
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
    OnDemand { id: submissionWindow; sourceComponent: Component { SubmissionWindow {
        app: window; submission: submissions; validationJobs: submissionJobs; specimenData: specimenMetadata
        accessionTableDialog: accessionTableDestination
    } } }
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

    OnDemand { id: orfDialog; sourceComponent: Component { OrfDialog { app: window } } }

    OnDemand { id: genesDialog; sourceComponent: Component { GenePredictionDialog { app: window } } }

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
        else evidenceSave.save()
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

    DocumentArea {
        id: documentArea
        app: window
        clipboard: clipboard; openCommand: openCommand; menu: menu; alnMenu: alnMenu; treeMenu: treeMenu
        renameDialog: renameDialog; tipDialog: tipDialog; cladeEditor: cladeEditor; editor: editor
        readOnlyCopy: readOnlyCopy; operationWindow: operationWindow
    }
    readonly property TabBar tabs: documentArea.tabs

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

    SequenceContextMenu { id: menu; app: window; editor: editor; clipboard: clipboard; addAnnotationCommand: addAnnotationCommand; orfDialog: orfDialog; genesDialog: genesDialog }

    AlignmentContextMenu { id: alnMenu; app: window; clipboard: clipboard; renameDialog: renameDialog }

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

    OnDemand { id: rootDialog; sourceComponent: Component { RootPositionDialog { app: window } } }

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

    OnDemand { id: supportName; sourceComponent: Component { SupportNameDialog { app: window } } }

    CladeEditor {
        id: cladeEditor
        controller: window.treeOpen ? window.current : null
    }
    OnDemand { id: treeStyle; sourceComponent: Component { TreeStyleDialog {
        id: treeStyleItem
        controller: window.treeOpen ? window.current : window.speciesTree
    } } }

    OnDemand { id: linkAlignmentFile; sourceComponent: Component { LinkAlignmentDialog { app: window } } }

    function exportSvg(format) {
        svgOptions.dialog().format = format || "svg"
        if (!window.current) return
        svgDialog.dialog().controller = window.current
        svgDialog.dialog().sourcePath = window.currentPath
        svgOptions.dialog().alignment = window.alignmentOpen
        svgOptions.dialog().open()
    }
    OnDemand { id: svgOptions; sourceComponent: Component { FigureExportOptionsDialog { app: window; fileDialog: svgDialog } } }

    OnDemand { id: svgDialog; sourceComponent: Component { FigureExportFileDialog { app: window } } }

    OnDemand { id: quitDialog; sourceComponent: Component { QuitDialog { app: window } } }

    OnDemand { id: confirmClose; sourceComponent: Component { CloseDocumentDialog { app: window } } }

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
