import QtQuick
import Basecraft

// The window the check tools drive (crates/basecraft-gui/tools/check_*.py,
// tools/test_delivery.py). The binary loads this instead of Main.qml when it
// gets --screenshot FILE, optionally with --window-width, --font-size,
// --font-points, --cpu-threads, --fit, --ribbon INDEX, --species-dialog
// METHOD, --distance-dialog FORM, --project-picker STEPS and the audits.
// It reaches the window's parts by objectName; nothing here is production.
Main {
    id: run
    scriptedRun: true

    function arg(name) {
        const args = Qt.application.arguments
        const i = args.indexOf(name)
        return i >= 0 ? args[i + 1] : undefined
    }
    function find(item, test) {
        if (!item) return null
        if (test(item)) return item
        for (const child of item.children || []) {
            const found = find(child, test)
            if (found) return found
        }
        return null
    }
    function named(name) { return find(run.header, i => i.objectName === name) || find(run.contentItem, i => i.objectName === name) }
    function part(name) { return run.contentData.find(item => item && item.objectName === name) }

    Component.onCompleted: {
        if (arg("--window-width")) width = Number(arg("--window-width"))
        if (arg("--font-size")) font.pixelSize = Number(arg("--font-size"))
    }

    // Exercise the actual project command and native picker for integration checks.
    Timer {
        id: projectPickerCheck
        interval: 150
        repeat: true
        property var steps: []
        property string previousAction: ""
        onTriggered: {
            if (previousAction.length) console.log("PROJECT_PICKER_AUDIT", JSON.stringify({
                action: previousAction, folder: run.projectDialog ? run.projectDialog.currentFolder.toString() : run.projectPickerFolder().toString(),
                policyFolder: run.projectPickerFolder().toString(),
                visible: run.projectDialog !== null && run.projectDialog.visible, project: run.part("shell").project_dir,
                historyError: run.part("preferences").project_error, preferencesError: run.part("preferences").error,
                preferencesReady: run.preferencesReady
            }))
            if (!steps.length) { stop(); Qt.quit(); return }
            const step = steps.shift()
            previousAction = step.action
            if (step.action === "open") run.applicationCommands.find(c => c.commandId === "project.open").trigger()
            else if (step.action === "browse" || step.action === "choose") run.projectDialog.selectedFolder = run.part("shell").local_file_url(step.path)
            else if (step.action === "accept") run.projectDialog.accept()
            else if (step.action === "reject") run.projectDialog.reject()
            else if (step.action === "select") run.selectProject(step.path)
        }
    }
    Timer {
        interval: 900
        running: true
        onTriggered: {
            const error = run.part("preferencesError")
            console.debug("PREFERENCES_AUDIT", JSON.stringify({error: run.part("preferences").error, modal: !!error.item && error.item.visible, ready: run.preferencesReady, font: run.font.family, fontPoints: run.font.pointSize, defaultFontPoints: run.defaultFontSizePoints}))
            if (run.arg("--project-picker")) {
                projectPickerCheck.steps = JSON.parse(run.arg("--project-picker"))
                projectPickerCheck.start()
                return
            }
            const c = run.current
            if (!c) { console.log("no document open"); shot.start(); return }
            if (run.arg("--cpu-threads")) run.localCpuThreads = Number(run.arg("--cpu-threads"))
            if (run.arg("--font-points")) run.part("preferences").update("fontSizePoints", run.arg("--font-points"))
            if (run.arg("--fit")) c.fit()
            if (run.arg("--ribbon")) run.ribbonTabName = run.ribbonTabs[Number(run.arg("--ribbon"))]
            if (run.arg("--species-dialog")) run.beginDelimitation(run.arg("--species-dialog"))
            if (run.arg("--distance-dialog")) run.part("toolSettings").dialog().show(c, "distance", run.arg("--distance-dialog"))
            shot.start()
        }
    }
    Timer {
        id: shot
        property bool accessibilityAudited: false
        interval: 400
        onTriggered: {
            const c = run.current
            if (c && c.busy) { shot.start(); return }
            const ribbon = run.named("applicationRibbon")
            if (run.arg("--accessibility-audit") && !accessibilityAudited) {
                accessibilityAudited = true
                const view = run.currentView
                const text = run.part("textRepresentation")
                const results = {editorFocused: view.activeFocus}
                view.pressKey(Qt.Key_F6, 0, "")
                results.ribbonFocused = run.named("ribbonTabBar").currentItem.activeFocus
                view.pressKey(Qt.Key_F6, Qt.ShiftModifier, "")
                results.editorReturned = view.activeFocus
                view.pressKey(Qt.Key_T, Qt.ControlModifier | Qt.AltModifier, "t")
                Qt.callLater(() => Qt.callLater(() => {
                    results.textOpened = text.dialog().opened
                    results.textFocused = text.dialog().initialFocusItem.activeFocus
                    results.hasText = text.dialog().snapshot.text.length > 0
                    view.pressKey(Qt.Key_F6, 0, "")
                    results.modalFocusKept = text.dialog().initialFocusItem.activeFocus
                    text.dialog().close()
                    Qt.callLater(() => {
                        results.focusRestored = view.activeFocus
                        console.log("ACCESSIBILITY_AUDIT", JSON.stringify(results))
                        shot.start()
                    })
                }))
                return
            }
            if (run.arg("--ribbon-audit")) {
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
                console.log("RIBBON_AUDIT", JSON.stringify({page: run.ribbonTab,
                    height: ribbon.height, contentWidth: ribbon.contentWidth,
                    viewportWidth: ribbon.viewportWidth, pageWidth: ribbon.pageWidth,
                    pageImplicitWidth: ribbon.currentPage.implicitWidth, spacing: ribbon.currentPage.spacing,
                    groups: groups, children: children}))
            }
            if (run.arg("--render-audit") && run.currentView) {
                const view = run.currentView
                const origin = view.mapToItem(null, 0, 0)
                console.log("RENDER_AUDIT", JSON.stringify({api: view.GraphicsInfo.api,
                    software: view.GraphicsInfo.api === GraphicsInfo.Software, error: view.renderError,
                    x: origin.x, y: origin.y, width: view.width, height: view.height,
                    windowWidth: run.width, windowHeight: run.height}))
            }
            console.log("local CPU threads", run.localCpuThreads)
            console.log("interface font points", run.font.pointSize)
            if (c) console.log(typeof c.memory_report === "function" ? c.memory_report() : "Trace", "|", c.status, "|", c.hscroll_size, c.vscroll_size)
            if (run.traceOpen && c.review_mode) console.log("trace mask", c.mask_settings, "thresholds", c.review_settings, "background", c.background, "hover status", run.traceDocument.hoverStatus)
            if (run.arg("--window-capture") && run.currentView)
                run.currentView.saveWindow(run.arg("--window-capture"))
            // Capture the scene into a texture: the offscreen platform's
            // window framebuffer grab is cropped at non-unit display scales.
            let scene = run.contentItem
            while (scene.parent) scene = scene.parent
            const started = scene.grabToImage(result => {
                console.log("saved", result.saveToFile(run.arg("--screenshot")))
                run.closeConfirmed = true
                Qt.quit()
            })
            if (!started) { console.error("Screenshot capture failed"); Qt.exit(1) }
        }
    }
}
