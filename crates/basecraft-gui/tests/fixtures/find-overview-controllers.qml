property bool findDone: findFlow.completed
Item {
    id: findFlow
    width: 400; height: 160
    property bool completed: false
    property string folder: FIND_TEST_FOLDER
    property int index: 0
    property int phase: 0
    readonly property var cases: [
        {controller: findGenome, file: "/sequence.fasta", query: "A"},
        {controller: findAlignment, file: "/alignment.afa", query: "specimen"},
        {controller: findTree, file: "/time-tree.nwk", query: "specimen"},
        {controller: findTrace, file: "/trace.ab1", query: "A"}
    ]
    readonly property var current: cases[index].controller
    GenomeController { id: findGenome }
    AlignmentController { id: findAlignment }
    TreeController { id: findTree }
    SangerController { id: findTrace }
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
        miniature: findFlow.current.overview_content
        matchMarks: JSON.parse(findFlow.current.overview_matches || "[]")
        viewportWidth: 0.5
        documentItem: findDocument
    }
    function require(value, message) {
        if (!value) { console.error("Find/overview controller " + index + ": " + message); Qt.exit(2) }
    }
    function startCase() {
        require(current.open_file(folder + cases[index].file), "fixture open failed")
        if (index === 3) current.cancel_calls()
        current.resize(160, 100)
        realFind.query = cases[index].query
        realFind.openFind()
        phase = 0
    }
    Component.onCompleted: startCase()
    Timer {
        interval: 20; repeat: true; running: !findFlow.completed
        onTriggered: {
            if (findFlow.phase === 0) {
                if (realFind.result.busy || realFind.result.count === 0) return
                const marks = realOverview.matchMarks
                findFlow.require(marks.length > 0, "accepted Find hits did not reach overview")
                for (const mark of marks) {
                    findFlow.require(isFinite(mark.x) && isFinite(mark.y) && mark.x >= 0 && mark.y >= 0 && mark.x + mark.width <= 1.000001 && mark.y + mark.height <= 1.000001, "mark outside normalized data")
                }
                if (findFlow.index < 3) {
                    const controller = findFlow.current
                    const identity = controller.file_path
                    const revision = controller.record_revision()
                    const marksBefore = controller.overview_matches
                    const wrongFile = findFlow.index === 0 ? "/alignment.afa" : "/sequence.fasta"
                    findFlow.require(!controller.open_file(findFlow.folder + wrongFile), "wrong-kind open reported success")
                    findFlow.require(controller.message.length > 0, "wrong-kind refusal has no explanation")
                    findFlow.require(controller.file_path === identity && controller.record_revision() === revision, "refusal replaced document identity or revision")
                    findFlow.require(controller.overview_matches === marksBefore, "refusal cleared Find selection")
                }
                const count = realFind.result.count
                realFind.poll(1)
                findFlow.require(realFind.result.current === (count > 1 ? 2 : 1), "next result did not follow shared Find order")
                findFlow.require(realOverview.matchMarks.length > 0, "navigation lost overview marks")
                realFind.query = "no-such-match!"
                realFind.submit()
                findFlow.require(JSON.parse(findFlow.current.overview_matches).length === 0, "query replacement retained stale overview marks")
                findFlow.phase = 1
            } else {
                if (realFind.result.busy) return
                findFlow.require(realFind.result.count === 0, "invalid/unmatched query unexpectedly matched")
                realFind.closeFind()
                findFlow.require(JSON.parse(findFlow.current.overview_matches).length === 0, "closing Find retained markers")
                if (findFlow.index === findFlow.cases.length - 1) {
                    findFlow.completed = true
                    console.log("Real Find/overview controller bindings passed")
                } else {
                    findFlow.index++
                    findFlow.startCase()
                }
            }
        }
    }
}
