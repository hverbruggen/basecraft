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
        for (const item of walk.contentData) if (item && item.objectName === name) return item
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
            const theme = find(welcome.contentItem, i => i.objectName === "firstTheme")
            check(theme, "first launch lost the theme question")
            theme.activated(2)
            check(preferenceValues.ui.theme === "dark", "theme answer not saved")
            const notices = find(welcome.contentItem, i => i.objectName === "firstReleaseNotices")
            check(notices && !notices.checked, "release notices must start off")
            theme.activated(0)
            welcome.close()
        })
        list.push(() => check(preferenceValues.ui.welcomed, "first launch would ask again"))
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
        list.push(() => { console.log("RIBBON_WALK passed"); Qt.quit() })
        return list
    }
    AppOperations { id: contactCheck }
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
