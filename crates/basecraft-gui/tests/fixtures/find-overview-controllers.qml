property bool findDone: findFlow.completed
Item {
    id: findFlow
    width: 400; height: 160
    property bool completed: false
    property string folder: FIND_TEST_FOLDER
    property int index: 0
    property int phase: 0
    property var beforeFind: ({})
    property int originalCount: 0
    readonly property bool hasOverview: index !== 4
    readonly property var cases: [
        {controller: findGenome, file: "/sequence.fasta", query: "A"},
        {controller: findAlignment, file: "/alignment.afa", query: "specimen"},
        {controller: findTree, file: "/time-tree.nwk", query: "specimen"},
        {controller: findTrace, file: "/trace.ab1", query: "A"},
        {controller: findWorkspace, file: "/hypotheses.spart", query: "specimen"}
    ]
    readonly property var current: cases[index].controller
    GenomeController { id: findGenome }
    AlignmentController { id: findAlignment }
    TreeController { id: findTree }
    SangerController { id: findTrace }
    DelimitationController { id: findWorkspace }
    Item { id: findDocument; width: 400; height: 90 }
    FindBar {
        id: realFind
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        controller: findFlow.current
        documentFocusItem: findDocument
    }
    DocumentOverview {
        id: realOverview
        width: 400; height: 30
        miniature: findFlow.hasOverview ? findFlow.current.overview_content : ""
        matchMarks: findFlow.hasOverview ? JSON.parse(findFlow.current.overview_matches || "[]") : []
        viewportWidth: 0.5
        documentItem: findDocument
    }
    function require(value, message) {
        if (!value) { console.error("Find/overview controller " + index + ": " + message); Qt.exit(2) }
    }
    function startCase() {
        require(current.open_file(folder + cases[index].file), "fixture open failed")
        if (index === 3) current.cancel_calls()
        if (hasOverview) current.resize(160, 100)
        if (index === 0 || index === 1) current.key(Qt.Key_A, "", Qt.ControlModifier)
        beforeFind = {dirty: current.dirty, revision: current.record_revision(),
            selection: index < 2 ? current.selection_text() : ""}
        // Replace a submitted scan before accepting its result.
        current.find_query("no-such-match!", false)
        current.find_poll(1)
        realFind.query = cases[index].query
        realFind.openFind()
        phase = 0
    }
    function finishCase() {
        realFind.closeFind()
        if (index === cases.length - 1) {
            completed = true
            console.log("Real Find/overview controller bindings passed")
        } else {
            index++
            startCase()
        }
    }
    Component.onCompleted: startCase()
    Timer {
        interval: 20; repeat: true; running: !findFlow.completed
        onTriggered: {
            if (findFlow.phase === 0) {
                if (realFind.result.busy || realFind.result.count === 0) return
                const marks = realOverview.matchMarks
                findFlow.require(!findFlow.hasOverview || marks.length > 0, "accepted Find hits did not reach overview")
                for (const mark of marks) {
                    findFlow.require(isFinite(mark.x) && isFinite(mark.y) && mark.x >= 0 && mark.y >= 0 && mark.x + mark.width <= 1.000001 && mark.y + mark.height <= 1.000001, "mark outside normalized data")
                }
                if (findFlow.index !== 3) {
                    const controller = findFlow.current
                    const identity = controller.file_path
                    const revision = controller.record_revision()
                    const marksBefore = findFlow.hasOverview ? controller.overview_matches : JSON.stringify(realFind.result.rows)
                    const wrongFile = findFlow.index === 0 ? "/alignment.afa" : "/sequence.fasta"
                    findFlow.require(!controller.open_file(findFlow.folder + wrongFile), "wrong-kind open reported success")
                    findFlow.require(controller.message.length > 0, "wrong-kind refusal has no explanation")
                    findFlow.require(controller.file_path === identity && controller.record_revision() === revision, "refusal replaced document identity or revision")
                    findFlow.require((findFlow.hasOverview ? controller.overview_matches : JSON.stringify(realFind.result.rows)) === marksBefore, "refusal cleared Find selection")
                }
                if (!findFlow.hasOverview) {
                    findFlow.require(JSON.stringify(realFind.result.rows) === "[0,1,2,3,4,5,6,7]", "workspace specimen order changed")
                    findFlow.require(realFind.result.label === "specimen_01", "workspace first label changed")
                }
                findFlow.require(findFlow.current.dirty === findFlow.beforeFind.dirty && findFlow.current.record_revision() === findFlow.beforeFind.revision, "Find changed document state")
                if (findFlow.index < 2) findFlow.require(findFlow.current.selection_text() === findFlow.beforeFind.selection, "Find replaced scientific selection")
                const count = realFind.result.count
                realFind.poll(1)
                findFlow.require(realFind.result.current === (count > 1 ? 2 : 1), "next result did not follow shared Find order")
                realFind.poll(-1)
                findFlow.require(realFind.result.current === 1, "previous did not return to first hit")
                realFind.poll(-1)
                findFlow.require(realFind.result.current === count, "previous did not wrap")
                if (!findFlow.hasOverview) findFlow.require(realFind.result.label === "specimen_08", "workspace last label changed")
                realFind.poll(1)
                findFlow.require(realFind.result.current === 1, "next did not wrap")
                findFlow.require(!findFlow.hasOverview || realOverview.matchMarks.length > 0, "navigation lost overview marks")
                realFind.query = "no-such-match!"
                realFind.submit()
                findFlow.require(!findFlow.hasOverview || JSON.parse(findFlow.current.overview_matches).length === 0, "query replacement retained stale overview marks")
                findFlow.phase = 1
            } else if (findFlow.phase === 1) {
                if (realFind.result.busy) return
                findFlow.require(realFind.result.count === 0, "invalid/unmatched query unexpectedly matched")
                realFind.closeFind()
                findFlow.require(!findFlow.hasOverview || JSON.parse(findFlow.current.overview_matches).length === 0, "closing Find retained markers")
                // Reopen the same path: a fresh Session must be accepted at the same revision.
                findFlow.require(findFlow.current.open_file(findFlow.folder + findFlow.cases[findFlow.index].file), "same-path replacement failed")
                if (findFlow.index === 3) findFlow.current.cancel_calls()
                realFind.query = findFlow.cases[findFlow.index].query
                realFind.openFind()
                findFlow.phase = 2
            } else if (findFlow.phase === 2) {
                if (realFind.result.busy || realFind.result.count === 0) return
                findFlow.require(realFind.result.current === 1, "replacement retained old result position")
                if (findFlow.index === 0) {
                    findFlow.originalCount = realFind.result.count
                    findGenome.key(Qt.Key_A, "", Qt.ControlModifier)
                    findGenome.key(Qt.Key_Delete, "", 0)
                    realFind.poll(0)
                    findFlow.phase = 3
                } else findFlow.finishCase()
            } else if (findFlow.phase === 3) {
                if (realFind.result.busy) return
                findFlow.require(realFind.result.count === 0, "edit retained old sequence hits")
                findGenome.undo()
                realFind.poll(0)
                findFlow.phase = 4
            } else {
                if (realFind.result.busy || realFind.result.count === 0) return
                findFlow.require(realFind.result.count === findFlow.originalCount, "undo did not restore Find hits")
                findFlow.finishCase()
            }
        }
    }
}
