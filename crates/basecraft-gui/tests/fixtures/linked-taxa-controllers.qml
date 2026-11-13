import QtQuick
import Basecraft
Main {
    id: test
    visible: false
    property string folder: LINKED_TEST_FOLDER
    property string hypothesis: LINKED_HYPOTHESIS
    property string group: LINKED_GROUP
    property var members: LINKED_MEMBERS
    property int stage: 0
    property bool began: false
    property QtObject a: null
    property QtObject duplicate: null
    property QtObject tree: null
    property QtObject workspace: null
    property QtObject unrelated: null
    property QtObject unlinked: null
    property var baselines: []
    property QtObject displayTree: null
    property string deliberateSelection: ""
    property var allNames: ["specimen_01", "specimen_02", "specimen_03", "specimen_04", "specimen_05", "specimen_06", "specimen_07", "specimen_08"]
    DocumentOpens { id: adapter }
    QtObject { id: unsupported }
    function check(ok, message) {
        if (!ok) { test.objectName = "stage " + stage + ": " + message; console.error("LINKED_TAXA stage " + stage + ": " + message); Qt.exit(2); throw new Error(message) }
    }
    // Rust checks these expectations against actual internal receiver state at
    // each event-loop boundary. The fixture never implements selection routing.
    function expect(controller, names) { controller.objectName = "linked-check:" + JSON.stringify(names.slice().sort()) }
    function record(controller) {
        return JSON.stringify([controller.document_record(), controller.record_revision(), controller.dirty])
    }
    function baseline() {
        baselines = [a, duplicate, tree, workspace, unrelated, unlinked, displayTree].map(c => ({controller: c, record: record(c)}))
    }
    function activate(path) { openPath(folder + path); refreshCurrent(false); check(current.file_path === folder + path, "activation failed") }
    function action(run) {
        const controller = current
        const focus = activeFocusItem
        const document = currentDocument
        const tab = ribbonTab
        run()
        check(current === controller && currentDocument === document && activeFocusItem === focus && ribbonTab === tab,
              "routing changed active document, focus or ribbon tab")
        for (const entry of baselines) check(record(entry.controller) === entry.record, "routing edited a document/record state")
    }
    function checkpoint() { stage++; test.objectName = "checkpoint:" + stage; Qt.quit() }
    function advance() {
        if (!began) return
        if (stage === 0) {
            if (!documentAt(0)) return
            if (a === null) {
                a = documentAt(0).controller
                documentOpens.add_tab(folder + "/alignment.afa", "alignment")
                documentOpens.add_tab(folder + "/tree.nwk", "tree")
                documentOpens.add_tab(folder + "/workspace.spart", "delimitation")
                documentOpens.add_tab(folder + "/unrelated.afa", "alignment")
                documentOpens.add_tab(folder + "/unlinked.nwk", "tree")
                duplicate = documentAt(1).controller
                tree = documentAt(2).controller
                workspace = documentAt(3).controller
                unrelated = documentAt(4).controller
                unlinked = documentAt(5).controller
                return
            }
            if (![a, duplicate, tree, workspace, unrelated, unlinked].every(c => c.file_path.length > 0)) return
            check(a !== duplicate && a.file_path === duplicate.file_path, "same-path controllers collapsed")
            check(documentAt(0).kind === "alignment" && documentAt(1).kind === "alignment" && documentAt(2).kind === "tree" && documentAt(3).kind === "delimitation", "view/controller kinds disagree")
            tree.link_alignment(folder + "/alignment.afa")
            check(tree.linked_alignment === a.file_path && workspace.linked_alignment === a.file_path, "real fixture associations failed")
            for (const c of [a, duplicate, tree, workspace, unrelated, unlinked]) { c.highlight_taxa('["sentinel"]'); expect(c, ["sentinel"]) }
            displayTree = documentAt(3).treeController
            check(displayTree.file_path === tree.file_path, "workspace display tree did not load")
            expect(displayTree, [])
            expect(workspace, []) // existing workspace receiver filters unknown specimen IDs
            baseline()
            // Barrier holds this adopted slot unavailable while events are routed.
            openPath(folder + "/pending.afa")
            check(documentAt(6) === null, "loading slot unexpectedly ready")
            activate("/alignment.afa")
            checkpoint()
        } else if (stage === 1) {
            action(() => {
                a.key(Qt.Key_A, "", Qt.ControlModifier)
                const selection = a.selection_text()
                a.key(Qt.Key_A, "", Qt.ControlModifier)
                check(a.selection_text() === selection && selection.length > 0, "repeat replaced deliberate selection")
                adapter.link_taxa(null, tree, "[]")
                adapter.link_taxa(a, null, "[]")
                adapter.link_taxa(a, a, "[]")
                adapter.link_taxa(unsupported, tree, "[]")
                adapter.link_taxa(a, unsupported, "[]")
                adapter.link_taxa(a, tree, "malformed")
            })
            deliberateSelection = a.selection_text()
            expect(tree, allNames); expect(workspace, allNames); expect(displayTree, allNames)
            check(documentAt(6) === null, "loading target became available")
            check(JSON.stringify(JSON.parse(workspace.selection_json).sort()) === JSON.stringify(allNames), "workspace did not receive names")
            checkpoint()
        } else if (stage === 2) {
            activate("/tree.nwk")
            action(() => { tree.select_clade('["specimen_01","specimen_02"]'); tree.select_clade('["specimen_01","specimen_02"]') })
            expect(a, ["specimen_01", "specimen_02"]); expect(duplicate, ["specimen_01", "specimen_02"])
            // Inactive workspace emissions must not become a second global source.
            action(() => workspace.highlight_taxa('["specimen_03"]'))
            expect(workspace, ["specimen_03"]); expect(displayTree, ["specimen_03"])
            check(a.selection_text() === deliberateSelection, "linked highlight replaced deliberate alignment selection")
            checkpoint()
        } else if (stage === 3) {
            activate("/workspace.spart")
            action(() => workspace.select_group(hypothesis, group, false))
            expect(workspace, members); expect(a, members); expect(duplicate, members); expect(displayTree, members)
            check(a.selection_text() === deliberateSelection, "workspace replaced deliberate alignment selection")
            check(speciesTree !== null, "workspace display-tree wiring absent")
            checkpoint()
        } else if (stage === 4) {
            action(() => { workspace.select_group(hypothesis, group, true); workspace.highlight_taxa("[]") })
            expect(workspace, []); expect(a, []); expect(duplicate, []); expect(displayTree, [])
            checkpoint()
        } else if (stage === 5) {
            activate("/alignment.afa")
            action(() => { a.key(Qt.Key_Escape, "", 0); a.taxaSelected("[]"); a.taxaSelected("[]") })
            expect(tree, []); expect(workspace, [])
            checkpoint()
        } else if (stage === 6) {
            // Retire the held request and a real target; subsequent indices shift.
            closeTab(6)
            baselines = baselines.filter(entry => entry.controller !== tree)
            tree = null
            closeTab(2)
            checkpoint()
        } else if (stage === 7) {
            activate("/alignment.afa")
            action(() => a.key(Qt.Key_A, "", Qt.ControlModifier))
            expect(workspace, allNames); expect(displayTree, allNames)
            openPath(folder + "/tree.nwk")
            checkpoint()
        } else if (stage === 8) {
            const reopened = documentAt(5)
            if (!reopened || currentPath !== folder + "/tree.nwk") return
            tree = reopened.controller
            check(tree.linked_alignment === a.file_path, "reopened association lost")
            expect(tree, [])
            baselines.push({controller: tree, record: record(tree)})
            activate("/alignment.afa")
            action(() => a.taxaSelected('["specimen_03"]'))
            expect(tree, ["specimen_03"]); expect(workspace, ["specimen_03"]); expect(displayTree, ["specimen_03"])
            checkpoint()
        }
    }
    Component.onCompleted: Qt.callLater(() => { began = true; openPath(folder + "/alignment.afa") })
    Timer {
        interval: 20; repeat: true; running: test.began
        onTriggered: {
            try { test.advance() }
            catch (error) { test.objectName = "stage " + test.stage + ": " + error; console.error(test.objectName); Qt.exit(2) }
        }
    }
    Timer { interval: 15000; running: true; onTriggered: { test.objectName = "timeout at stage " + test.stage; console.error("LINKED_TAXA", test.objectName); Qt.exit(3) } }
}
