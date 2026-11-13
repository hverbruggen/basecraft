import QtQuick
import Basecraft

// The production window with one real document of each kind: every ribbon
// tab of every kind must show its page at wide, narrow and scaled sizes, the
// Export tab keeps its commands, and key hints reach real commands. The
// parent process fails the run on any QML warning.
Main {
    id: walk
    visible: true
    width: 1600; height: 900
    property string folder: RIBBON_TEST_FOLDER
    readonly property var files: ["/locus.gb", "/homologues.afa", "/time.nwk", "/hypotheses.spart", "/forward.ab1"]
    readonly property var kinds: ["sequence", "alignment", "tree", "delimitation", "trace"]
    readonly property var sizes: [[1600, 100], [320, 100], [640, 150], [800, 200]]
    property var steps: []
    property bool planned: false
    property int waited: 0

    function fail(message) { console.error("RIBBON_WALK failed: " + message); Qt.exit(2) }
    function check(value, message) { if (!value) fail(message) }
    // A step that waits: it runs again on the next tick until test() holds.
    function until(test, message) {
        let tries = 0
        const step = () => { if (!test()) { if (++tries > 1500) fail(message); walk.steps.unshift(step) } }
        return step
    }
    function tabOf(path) {
        for (let i = 0; i < documentTabs().count; i++) {
            const d = documentAt(i)
            if (d && d.controller && d.controller.file_path === path) return i
        }
        return -1
    }
    function textOf(controller) { controller.select_all(); return controller.selection_text() }
    function find(item, test) {
        if (!item) return null
        if (test(item)) return item
        for (const child of item.children || []) {
            const found = find(child, test)
            if (found) return found
        }
        return null
    }
    function all(item, test, into) {
        if (item && item.visible) {
            if (test(item)) into.push(item)
            for (const child of item.children || []) all(child, test, into)
        }
        return into
    }
    function named(name) {
        for (const root of [walk.header, walk.contentItem]) {
            const found = find(root, i => i.objectName === name)
            if (found) return found
        }
        return null
    }
    function ribbonItem() { return named("applicationRibbon") }
    function documentTabs() { return named("documentTabs") }
    function show(index) { documentTabs().currentIndex = index; refreshCurrent(false) }
    function hintFor(id) {
        const target = ribbonNavigation.targets.find(t => { const c = t.item.commandAction || t.item.action || t.item; return c.commandId === id })
        check(target, "no key hint for " + id)
        ribbonNavigation.choose(target)
    }
    // Visit a document's tab, then each of its ribbon tabs next.
    function visit(index) {
        show(index)
        const kind = kinds[index]
        const pages = []
        for (const name of ribbonTabs) {
            pages.push(() => { ribbonTabName = name })
            pages.push(() => checkPage(kind, name))
        }
        steps = pages.concat(steps)
    }

    // One check per tick, so layouts settle between them.
    function checkPage(kind, name) {
        check(currentKind === kind, "tab of " + kind + " shows " + currentKind)
        check(ribbonTab === name, kind + ": tab " + name + " not shown")
        const page = ribbonItem().currentPage
        check(page && page.visible, kind + "/" + name + ": no page")
        const id = i => i.commandId !== undefined ? i.commandId : i.action && i.action.commandId !== undefined ? i.action.commandId : null
        const buttons = all(page, i => id(i) !== null && i.width !== undefined, [])
        check(buttons.length > 0, kind + "/" + name + ": no commands")
        for (const button of buttons)
            check(button.width > 0 && button.height > 0, kind + "/" + name + ": " + id(button) + " has no size")
    }
    function checkExport() {
        const page = ribbonItem().currentPage
        const svg = find(page, i => i.text === "SVG" && i.action !== undefined && i.action !== null)
        const png = find(page, i => i.text === "PNG" && i.action !== undefined && i.action !== null)
        check(svg && png, "Export lost its SVG and PNG buttons")
        check(svg.action.commandId === "ribbon.export-an-editable-svg-of-this-view", "SVG runs " + svg.action.commandId)
        check(png.action.commandId === "figure.export-png", "PNG runs " + png.action.commandId)
        for (const id of ["metadata.table", "metadata.import-headers", "metadata.import-sheet", "metadata.template", "submission.new", "submission.open", "submission.prepare", "submission.validate", "submission.submitter"])
            check(find(page, i => i.commandId === id), "Export lost " + id)
        check(named("ribbonFigureExport"), "utility figure export missing")
    }
    function command(id) {
        const found = applicationCommands.find(c => c.commandId === id)
        check(found, "no command " + id)
        return found
    }
    function dialog(name) {
        for (const item of walk.contentData) {
            if (item && item.objectName === name) return item
            if (item && item.item && item.item.objectName === name) return item.item
        }
        fail("no dialog " + name)
    }
    function hint(key) {
        const target = ribbonNavigation.targets.find(t => t.key === key)
        check(target, "no key hint " + key + " at level " + ribbonNavigation.level)
        ribbonNavigation.choose(target)
    }

    function plan() {
        const list = []
        // A new profile is asked the first-launch questions once; answers
        // apply at once and closing records that they were asked.
        list.push(() => {
            const welcome = dialog("firstLaunch").item
            check(welcome && welcome.opened, "first launch did not ask its questions")
            const theme = find(welcome.contentItem, i => i.objectName === "theme")
            check(theme, "first launch lost the theme question")
            theme.activated(2)
            check(preferenceValues.ui.theme === "dark", "theme answer not saved")
            const family = find(welcome.contentItem, i => i.objectName === "fontFamily")
            check(family && family.currentIndex === 0 && family.count > 1, "first launch lost the font question")
            family.activated(1)
            check(preferenceValues.ui.font_family === family.families[0], "font answer not saved")
            family.activated(0)
            check(preferenceValues.ui.font_family === null, "desktop font not restored")
            const notices = find(welcome.contentItem, i => i.objectName === "releaseNotices")
            check(notices && !notices.checked, "release notices must start off")
            const cpu = find(welcome.contentItem, i => i.objectName === "localCpuThreads")
            check(cpu && !cpu.visible, "first launch asks more than appearance")
            theme.activated(0)
            welcome.close()
        })
        list.push(() => check(preferenceValues.ui.welcomed, "first launch would ask again"))
        // Preferences and the shortcut list opened and closed twice, then the
        // window and ribbon resized: rows built and removed with the dialog
        // must not outlive their layout (this crashed the window).
        for (let pass = 0; pass < 2; ++pass) {
            list.push(() => command("application.preferences").trigger())
            list.push(() => {
                const form = walk.contentData.find(i => i && i.item && i.item.title === "Preferences")
                check(form && form.item.opened, "Preferences did not open")
                check(find(form.item.contentItem, i => i.objectName === "shortcut-file.save"), "Preferences lost its shortcut rows")
                form.item.close()
            })
            list.push(() => command("help.shortcuts").trigger())
            list.push(() => {
                const open = walk.contentData.filter(item => item && item.item && item.item.opened)
                check(open.length === 1 && find(open[0].item.contentItem, i => i.objectName === "shortcutGroups"), "the shortcut list did not open")
                open[0].item.close()
            })
        }
        list.push(() => { walk.width = 320; persistPreference("ribbonSizePercent", 150) })
        list.push(() => { walk.width = 1600; persistPreference("ribbonSizePercent", 100) })
        // One contact-details dialog: the submitter's email serves BLAST
        // until BLAST gets its own.
        list.push(() => { command("submission.submitter").trigger() })
        list.push(() => {
            const contacts = dialog("submitterDialog")
            check(contacts.opened, "contact details did not open")
            const field = name => find(contacts.contentItem, i => i.objectName === name)
            for (const [name, text] of [["submitterFirstName", "Jane"], ["submitterLastName", "Doe"], ["submitterEmail", "jane@example.org"],
                ["submitterInstitution", "Museum"], ["submitterCity", "Leiden"], ["submitterCountry", "Netherlands"]]) field(name).text = text
            contacts.accept()
        })
        list.push(() => {
            check(contactCheck.blast_contact() === "jane@example.org", "BLAST did not take the submission email")
            command("submission.submitter").trigger()
        })
        list.push(() => {
            const contacts = dialog("submitterDialog")
            find(contacts.contentItem, i => i.objectName === "blastEmail").text = "lab@example.org"
            contacts.accept()
        })
        list.push(() => check(contactCheck.blast_contact() === "lab@example.org", "BLAST email not kept separately"))
        for (const [width, scale] of sizes) {
            list.push(() => { walk.width = width; persistPreference("ribbonSizePercent", scale) })
            for (let index = 0; index < kinds.length; ++index) list.push(() => visit(index))
        }
        list.push(() => { walk.width = 1600; persistPreference("ribbonSizePercent", 100); show(0); ribbonTabName = "Export" })
        list.push(checkExport)
        // F10, Alignment, Overwrite: the real binding switches insert mode off.
        list.push(() => { show(1); ribbonTabName = "File"; ribbonNavigation.start(); hint("A") })
        list.push(() => {
            check(ribbonNavigation.level === 2, "Alignment hints did not open")
            const before = alignmentInsert
            hintFor("ribbon.overwrite-typing-replaces-the-selected-bases")
            check(before && !alignmentInsert, "Overwrite hint did not switch insert mode off")
            alignmentInsert = true
        })
        // A disabled command explains itself through the same hints.
        list.push(() => { ribbonNavigation.start(); hint("A") })
        list.push(() => {
            const disabled = ribbonNavigation.targets.find(t => !t.item.enabled)
            if (disabled) {
                ribbonNavigation.choose(disabled)
                check(ribbonNavigation.explanation.length > 0, "disabled command has no explanation")
            }
            ribbonNavigation.stop(false)
        })
        // D13, before a project is chosen: with one, every session keeps its
        // recovery in the project's .basecraft. Here the files are loose.
        // The first edit in a folder that cannot hold .basecraft is refused.
        // The window offers a copy; Cancel leaves the file unedited; Save a copy
        // copies it where the user chooses, opens the copy in its place and
        // redoes the edit there. Every kind of editor makes the offer.
        const locked = folder + "-locked/locked.afa"
        const copied = folder + "/copied.afa"
        const offer = () => dialog("readOnlyCopyDialog")
        const tabPaths = () => { const out = []; for (let i = 0; i < documentTabs().count; i++) { const d = documentAt(i); out.push(d && d.controller ? d.controller.file_path : "?") } return out.join(", ") }
        const opened = path => until(() => tabOf(path) >= 0 && documentAt(tabOf(path)).controller, path + " did not open; open: " + tabPaths())
        const lockedTabs = {}
        list.push(() => { lockedTabs.count = documentTabs().count; openPath(locked) })
        list.push(opened(locked))
        list.push(() => {
            show(tabOf(locked))
            const ctl = documentAt(tabOf(locked)).controller
            ctl.select_block(0, 0, 0, 0)
            ctl.insert_gap()
            check(offer() && offer().opened && !ctl.dirty, "a refused edit in a read-only folder did not offer a copy")
            offer().reject()
            check(!offer().opened && ctl.refused_edit === "" && !ctl.dirty, "cancelling the copy offer did not leave the file unedited")
            ctl.insert_gap()
            check(offer().opened, "the offer did not come back for the next edit")
            find(offer().contentItem, i => i.objectName === "saveEditableCopy").clicked()
            const save = dialog("saveDocumentDialog")
            check(save && save.visible, "Save a copy did not open the file dialog")
            // The Quick file dialog accepts what its own name field holds, so
            // the journey hands the chosen path to the dialog's continuation.
            const next = save.continuation
            save.reject()
            // The file dialog is its own window; a desktop reactivates ours after it.
            walk.requestActivate()
            next(copied)
        })
        list.push(opened(copied))
        list.push(() => {
            const ctl = documentAt(tabOf(copied)).controller
            check(tabOf(locked) < 0 && ctl.dirty && ctl.refused_edit === "", "the copy did not replace the original with the edit redone")
            show(tabOf(copied))
            currentView.forceActiveFocus()
            currentView.pressKey(Qt.Key_S, Qt.ControlModifier, "")
            edited.controller = ctl
        })
        list.push(until(() => !edited.controller.dirty, "the edited copy did not save"))
        // Cancelling the close question keeps the tab; Discard closes it.
        list.push(() => {
            edited.controller.insert_gap()
            currentView.pressKey(Qt.Key_W, Qt.ControlModifier, "")
            const ask = dialog("closeDocumentDialog")
            check(ask && ask.opened, "closing unsaved work did not ask")
            ask.reject()
            check(tabOf(copied) >= 0 && edited.controller.dirty, "cancelling the close question closed the tab")
            currentView.pressKey(Qt.Key_W, Qt.ControlModifier, "")
            dialog("closeDocumentDialog").discarded()
            check(tabOf(copied) < 0, "discarding did not close the tab")
        })
        // The sequence and tree editors make the same offer.
        for (const [name, edit] of [["locus.gb", ctl => ctl.delete_feature(0)],
                                    ["time.nwk", ctl => { ctl.select_clade(JSON.stringify(["specimen_01", "specimen_02"])); const a = JSON.parse(ctl.annotation_json()); a.label = "AB"; ctl.set_annotation(JSON.stringify(a)) }]]) {
            const path = folder + "-locked/" + name
            list.push(() => openPath(path))
            list.push(opened(path))
            list.push(() => {
                show(tabOf(path))
                const ctl = documentAt(tabOf(path)).controller
                edit(ctl)
                check(offer().opened && !ctl.dirty, name + ": a refused edit did not offer a copy")
                offer().reject()
                check(!ctl.dirty, name + ": cancelling edited the file")
                closeTab(tabOf(path))
            })
        }
        // A trace review in a read-only folder is refused when it is created;
        // the offer reopens the review on a copy.
        const trace = folder + "-locked/forward.ab1"
        const traceCopy = folder + "/forward-copy.ab1"
        list.push(until(() => documentAt(4) && !documentAt(4).controller.busy, "the first trace did not finish processing"))
        list.push(() => openPath(trace))
        list.push(until(() => offer() && offer().opened, "a refused trace review did not offer a copy; tabs " + tabPaths()))
        list.push(() => {
            find(offer().contentItem, i => i.objectName === "saveEditableCopy").clicked()
            const save = dialog("saveDocumentDialog")
            const next = save.continuation
            save.reject()
            // The file dialog is its own window; a desktop reactivates ours after it.
            walk.requestActivate()
            next(traceCopy)
        })
        list.push(until(() => documentTabs().count === lockedTabs.count + 1 && !offer().opened, "the trace copy did not replace the original"))
        list.push(() => closeTab(documentTabs().count - 1))
        // D12: a sequence basecraft cannot place asks what it is, once.
        const unclear = folder + "/unclear.fasta"
        list.push(() => openPath(unclear))
        list.push(opened(unclear))
        // D22: Cancel means not now; the question comes back only on reopen.
        list.push(() => {
            show(tabOf(unclear))
            const ask = dialog("alphabetDialog")
            check(ask && ask.opened && ask.question.includes("unclear"), "an undetermined sequence did not ask what it is")
            ask.reject()
            show(0)
            show(tabOf(unclear))
            check(!dialog("alphabetDialog").opened, "a cancelled question came back while the file stayed open")
            closeTab(tabOf(unclear))
        })
        list.push(() => openPath(unclear))
        list.push(opened(unclear))
        list.push(() => {
            show(tabOf(unclear))
            const ask = dialog("alphabetDialog")
            check(ask && ask.opened && ask.question.includes("unclear"), "reopening did not ask again after Cancel")
            find(ask.contentItem, i => i.objectName === "alphabetDna").clicked()
            const ctl = documentAt(tabOf(unclear)).controller
            // D22: the answer is in the side file at once, so nothing is left to save.
            check(!ask.opened && ctl.alphabet_question === "" && !ctl.dirty, "stating DNA was not recorded at once")
            check(documentAt(1).controller.status.includes("DNA (guessed)"), "the alignment status does not name its guessed alphabet: " + documentAt(1).controller.status)
            currentView.pressKey(Qt.Key_W, Qt.ControlModifier, "")
        })
        // Sequence commands: defaults, a remapped shortcut, document context.
        list.push(() => {
            show(0)
            check(command("file.new-sequence").shortcut === "Ctrl+N", "new sequence lost Ctrl+N")
            const annotate = command("ribbon.annotate-the-selection")
            check(annotate.shortcut === "Ctrl+Shift+N" && annotate.enabled, "annotate not ready on a sequence")
            persistPreference("shortcut", {id: "ribbon.annotate-the-selection", value: "Ctrl+J"})
        })
        list.push(() => {
            check(command("ribbon.annotate-the-selection").shortcut === "Ctrl+J", "remapped shortcut not applied")
            persistPreference("shortcut", {id: "ribbon.annotate-the-selection", value: null})
            show(1)
        })
        list.push(() => {
            const annotate = command("ribbon.annotate-the-selection")
            check(annotate.shortcut === "Ctrl+Shift+N" && !annotate.enabled, "annotate active outside sequences")
            const methods = command("provenance.methods")
            check(methods.enabled && methods.text === "Methods", "methods unavailable with a document")
            methods.trigger()
        })
        list.push(() => {
            const record = dialog("documentRecordDialog")
            check(record.opened && record.page === 1 && !record.savedRevision, "methods did not open the current record")
            record.close()
            // A missing project folder changes nothing; an existing one is chosen.
            selectProject(folder + "/missing")
            check(dialog("preferences").project_error.includes("/missing") && metadataProject !== folder + "/missing", "missing project accepted")
            selectProject(folder)
            check(metadataProject === folder && dialog("preferences").recent_projects.includes(folder) && dialog("preferences").project_error === "", "project not chosen")
        })
        // Figure export options: PNG width, selection for alignments only.
        list.push(() => { show(1); exportSvg("png") })
        list.push(() => {
            const options = dialog("figureExportOptions")
            const width = find(options.contentItem, i => i.objectName === "pngWidth")
            const selection = find(options.contentItem, i => i.objectName === "svgSelection")
            check(options.opened && width.visible && width.value === 1500, "PNG export lost its width")
            check(selection.visible && !selection.checked, "alignment export lost the selection option")
            options.reject()
            show(2)
            exportSvg("svg")
        })
        list.push(() => {
            const options = dialog("figureExportOptions")
            check(options.opened, "SVG export options did not open")
            check(!find(options.contentItem, i => i.objectName === "pngWidth").visible, "SVG export shows a PNG width")
            check(!find(options.contentItem, i => i.objectName === "svgSelection").visible, "tree export offers selection")
            options.reject()
        })
        // The colour-blind palette is a View ribbon toggle that saves the preference.
        list.push(() => { show(0); ribbonTabName = "View" })
        list.push(() => {
            const palette = find(ribbonItem().currentPage, i => i.objectName === "colorBlindFriendly")
            check(palette && palette.visible, "View lost the colour-blind toggle")
            const before = palette.checked
            palette.toggle(); palette.toggled()
            check((preferenceValues.ui.data_color_scheme === "color-blind-friendly") === !before, "toggle did not save the palette")
            palette.toggle(); palette.toggled()
            check(palette.checked === before, "toggle did not restore the palette")
        })
        // Dialogs built on first use open from their commands.
        for (const id of ["application.preferences", "help.shortcuts", "tools.installed-operations"]) {
            list.push(() => {
                check(command(id).enabled, id + " unavailable")
                command(id).trigger()
            })
            list.push(() => {
                const open = walk.contentData.filter(item => item && item.item && item.item.opened)
                check(open.length === 1, id + " did not open its dialog")
                open[0].item.close()
            })
        }
        // Ctrl+A belongs to the canvas only while it can draw (offscreen runs
        // use the software renderer, so it passes keys on); a text field
        // always selects its own text and leaves the sequence alone.
        list.push(() => { show(0); currentView.forceActiveFocus() })
        list.push(() => {
            currentView.pressKey(Qt.Key_A, Qt.ControlModifier, "a")
            check((current.selection_text().length > 0) === currentView.renderingAvailable, "Ctrl+A in the canvas went to the wrong place")
            documentAt(0).openFind()
        })
        list.push(() => {
            const query = find(documentAt(0), i => i.objectName === "findQuery")
            check(query && query.activeFocus, "the find field did not take focus")
            query.text = "entry text"
            currentView.pressKey(Qt.Key_A, Qt.ControlModifier, "a")
            check(query.selectedText === "entry text", "Ctrl+A in a text field did not select its text")
            query.text = ""
            currentView.pressKey(Qt.Key_Escape, 0, "")
        })
        // The trace review's coordinate box moves to a column in the chosen
        // system, refuses a bad one in place, and the evidence panel folds.
        list.push(() => { show(4) })
        list.push(() => {
            const trace = documentAt(4)
            const input = find(trace, i => i.objectName === "reviewCoordinateInput")
            const panel = find(trace, i => i.objectName === "reviewNavigation")
            check(input && panel && panel.visible, "the trace review lost its coordinate box")
            input.forceActiveFocus()
            input.text = "outside"
            trace.goToColumn()
            check(input.activeFocus, "a bad coordinate left the coordinate box")
            find(trace, i => i.objectName === "reviewCoordinateSystem").currentIndex = 1
            input.text = "1"
            input.accepted()
            check(currentView.activeFocus, "going to a draft column did not return to the canvas")
            const toggle = find(trace, i => i.objectName === "reviewEvidenceToggle")
            const evidence = find(trace, i => i.objectName === "reviewEvidence")
            check(evidence.visible, "site evidence hidden by default")
            toggle.checked = false
            check(!evidence.visible, "site evidence did not fold")
            toggle.checked = true
        })
        // The species menu says "Rename species…" once the selection is an
        // existing species, and "Name species…" before.
        list.push(() => {
            const view = documentAt(3)
            const ctl = view.controller
            const hypothesis = view.hypotheses.find(h => (h.groups || []).length > 0)
            check(hypothesis, "the species workspace has no groups")
            ctl.active_hypothesis = hypothesis.id
            const menu = view.data.find(o => o && o.objectName === "speciesTreeMenu")
            const caption = () => {
                menu.openFor({})
                let text = ""
                for (let i = 0; i < menu.count; i++) if (menu.itemAt(i).objectName === "nameSpecies") text = menu.itemAt(i).text
                menu.close()
                return text
            }
            ctl.highlight_taxa("[]")
            check(caption() === "Name species…", "species menu renames an empty selection")
            ctl.select_group(hypothesis.id, hypothesis.groups[0].id, false)
            check(caption() === "Rename species…", "species menu did not follow the selection")
        })
        // Every dialog built on demand builds without a QML warning.
        list.push(() => {
            const loaders = walk.contentData.filter(item => item && item.sourceComponent !== undefined && typeof item.dialog === "function")
            check(loaders.length >= 50, "only " + loaders.length + " dialogs load on demand")
            for (const loader of loaders) check(loader.dialog(), "a dialog failed to build")
        })
        // D11: a reroot that displaces clade labels asks where each goes. On the
        // rooted tree [&R] ((a,b),(c,d)) labelled "All" and "AB", rooting on b
        // leaves "All" with an exact clade (the root) and "AB" without one.
        const rooted = folder + "/rooted.nwk"
        const tree = {}
        const button = name => find(dialog("labelPlacementDialog").contentItem, i => i.objectName === name)
        const labelClade = (tips, label) => {
            tree.ctl.select_clade(JSON.stringify(tips))
            const annotation = JSON.parse(tree.ctl.annotation_json())
            annotation.label = label
            check(tree.ctl.set_annotation(JSON.stringify(annotation)) === "", "could not label " + label)
        }
        const reroot = () => { tree.ctl.select_clade(JSON.stringify(["b"])); tree.ctl.reroot() }
        const question = () => tree.ctl.label_question.length ? JSON.parse(tree.ctl.label_question) : null
        list.push(() => openPath(rooted))
        list.push(until(() => tabOf(rooted) >= 0 && documentAt(tabOf(rooted)).controller, "the rooted tree did not open"))
        list.push(() => {
            show(tabOf(rooted))
            tree.ctl = documentAt(tabOf(rooted)).controller
            labelClade(["a", "d"], "All")
            labelClade(["a", "b"], "AB")
            reroot()
            const ask = dialog("labelPlacementDialog")
            const q = question()
            check(ask && ask.opened && q && q.label === "All" && q.number === 1 && q.total === 2, "the reroot did not ask about its first displaced label: " + tree.ctl.label_question)
            check(button("labelExact").visible && button("labelPlacementAll").visible, "the exact clade or apply-to-all was not offered")
            check(!tree.ctl.status.includes("(moved)"), "labels are still marked as moved")
            button("labelExact").clicked()
            const next = question()
            check(ask.opened && next && next.label === "AB" && !button("labelExact").visible, "the second label offers an exact clade it does not have")
            // Escape undoes the placement made so far and the reroot (D11 has
            // no "leave it there"); the tree is as before.
            ask.reject()
            check(tree.ctl.label_question === "" && !tree.ctl.status.includes("moved labels"), "Escape did not undo the reroot")
            // Answer for every label at once.
            currentView.forceActiveFocus()
            reroot()
            check(ask.opened, "rerooting again did not ask")
            button("labelPlacementAll").checked = true
            check(!button("labelExact").visible, "exact offered for every label though AB has no exact clade")
            button("labelSmallest").clicked()
            check(!ask.opened && tree.ctl.label_question === "" && !tree.ctl.status.includes("moved labels"), "one answer did not place every label")
            // Undo reroot from the question undoes the reroot.
            currentView.pressKey(Qt.Key_Z, Qt.ControlModifier, "")
            currentView.pressKey(Qt.Key_Z, Qt.ControlModifier, "")
            reroot()
            button("labelUndo").clicked()
            check(!ask.opened && !tree.ctl.status.includes("moved labels"), "Undo reroot left displaced labels")
            currentView.pressKey(Qt.Key_W, Qt.ControlModifier, "")
            dialog("closeDocumentDialog").discarded()
            check(tabOf(rooted) < 0, "the rooted tree did not close")
        })
        // 6.1: open, edit, save, close and reopen an alignment, with undo, redo,
        // save and close from the keyboard (window shortcuts, through the
        // platform's key entry). The parent process checks the saved file.
        const aligned = folder + "/homologues.afa"
        const edited = {}
        list.push(() => {
            show(1)
            const ctl = documentAt(1).controller
            check(ctl.file_path === aligned && !ctl.dirty, "the alignment opened dirty or under another path")
            edited.original = textOf(ctl)
            ctl.select_block(0, 0, 0, 0)
            ctl.insert_gap()
            edited.text = textOf(ctl)
            check(ctl.dirty && edited.text !== edited.original, "inserting a gap changed nothing")
            currentView.forceActiveFocus()
            currentView.pressKey(Qt.Key_Z, Qt.ControlModifier, "")
            check(textOf(ctl) === edited.original, "Ctrl+Z did not undo the gap")
            currentView.pressKey(Qt.Key_Z, Qt.ControlModifier | Qt.ShiftModifier, "")
            check(textOf(ctl) === edited.text, "Ctrl+Shift+Z did not redo the gap")
            edited.controller = ctl
            currentView.pressKey(Qt.Key_S, Qt.ControlModifier, "")
        })
        list.push(until(() => !edited.controller.dirty, "Ctrl+S did not save the alignment"))
        list.push(() => {
            const ctl = edited.controller
            check(documentAt(1).controller === ctl && ctl.file_path === aligned && documentTabs().currentIndex === 1, "saving changed the document's identity or tab")
            edited.tabs = documentTabs().count
            currentView.pressKey(Qt.Key_W, Qt.ControlModifier, "")
            check(documentTabs().count === edited.tabs - 1 && tabOf(aligned) < 0, "Ctrl+W did not close the saved alignment")
            openPath(aligned)
        })
        list.push(until(() => tabOf(aligned) >= 0 && documentAt(tabOf(aligned)).controller, "the alignment did not reopen"))
        list.push(() => {
            const ctl = documentAt(tabOf(aligned)).controller
            check(ctl !== edited.controller && !ctl.dirty && textOf(ctl) === edited.text, "the reopened alignment lost the saved gap")
        })
        // 6.2 background completion: an annotator run on the sequence finishes
        // while another tab is active, without moving the tab or the focus. A
        // project switch meanwhile is refused before any question.
        list.push(() => {
            show(0)
            edited.controller = documentAt(0).controller
            edited.controller.find_orfs(30, false)
            check(edited.controller.busy, "the ORF finder did not start")
            show(2)
            currentView.forceActiveFocus()
            edited.focus = activeFocusItem
            selectProject(folder + "/second")
            check(dialog("preferences").project_error.startsWith("A task is still running") && metadataProject === folder && documentTabs().count === edited.tabs, "a busy tab did not refuse the switch first")
        })
        list.push(until(() => !edited.controller.busy, "the ORF finder did not finish"))
        list.push(() => {
            check(documentTabs().currentIndex === 2 && activeFocusItem === edited.focus, "a background completion moved the tab or the focus")
            check(edited.controller.message.length > 0, "the ORF finder reported nothing")
        })
        // Cancellation through the job service: a plugin run that never ends
        // is cancelled from the job list; the focus stays where it was.
        list.push(() => {
            check(hangRun.submit(folder, JSON.stringify({operation: "org.example.registry-fixture/count@1",
                inputs: {sources: [{kind: "file", path: "time.nwk"}]}, parameters: {mode: "hang"}})), "the hanging run did not start")
        })
        list.push(until(() => JSON.parse(jobWatch.jobs).some(j => j.operation.includes("registry-fixture") && j.status === "running"), "the hanging run is not in the job list"))
        list.push(() => {
            selectProject(folder + "/second")
            check(dialog("preferences").project_error.startsWith("A task is still running"), "a running job did not refuse the switch")
            const row = JSON.parse(jobWatch.jobs).find(j => j.operation.includes("registry-fixture") && j.status === "running")
            check(row.cancellable, "the running plugin cannot be cancelled")
            jobWatch.cancel(row.id)
        })
        list.push(until(() => !hangRun.busy && JSON.parse(jobWatch.jobs).some(j => j.operation.includes("registry-fixture") && j.status === "cancelled"), "cancelling from the job list did not stop the run"))
        list.push(() => check(activeFocusItem === edited.focus && documentTabs().currentIndex === 2, "the cancelled job moved the focus"))
        // Choosing another project asks about unsaved work as quitting does,
        // closes every document, then switches.
        list.push(() => {
            documentAt(0).controller.dirty = true
            selectProject(folder + "/second")
            const ask = dialog("quitDocumentsDialog")
            check(ask.opened && metadataProject === folder, "switching project did not ask about unsaved work")
            ask.discarded()
            check(documentTabs().count === 0, "switching project left documents open")
        })
        list.push(until(() => metadataProject === folder + "/second" || dialog("preferences").project_error.length > 0, "the project choice never finished"))
        list.push(() => {
            const error = dialog("preferences").project_error
            check(metadataProject === folder + "/second" && error === "", "project did not switch after its documents closed: " + error)
        })
        list.push(() => { console.log("RIBBON_WALK passed"); Qt.quit() })
        return list
    }
    AppOperations { id: contactCheck }
    AppOperations { id: hangRun }
    AppJobs { id: jobWatch }
    Timer {
        interval: 30; repeat: true; running: true
        onTriggered: {
            if (!walk.planned) {
                // Wait until every document is open in its tab.
                if (++walk.waited > 600) walk.fail("documents did not open")
                if (walk.documentTabs().count !== walk.files.length) return
                for (let i = 0; i < walk.files.length; ++i) if (!walk.documentAt(i)) return
                walk.steps = walk.plan()
                walk.planned = true
                return
            }
            if (!walk.steps.length) return
            try { walk.steps.shift()() } catch (error) { walk.fail(error + "") }
        }
    }
    Component.onCompleted: { for (const file of files) openPath(folder + file) }
}
