property bool speciesDone: figures.completed && !trace.busy
Item {
    id: figures
    property bool completed: false
    property int opening: 6
    function ready(success) {
        if (!success) { console.error("Figure open failed"); Qt.exit(2); return }
        opening--
        if (!opening) runFigures()
    }
    property string savedPreview: ""
    property string savedWorkspace: ""
    property string savedDisplay: ""
    property string folder: FIGURE_TEST_FOLDER
    GenomeController { id: genome; onOpened: success => figures.ready(success) }
    AlignmentController { id: alignment; onOpened: success => figures.ready(success) }
    TreeController { id: tree; onOpened: success => figures.ready(success) }
    DelimitationController { id: delimitation; onOpened: success => figures.ready(success) }
    SangerController { id: trace }
    MetadataController { id: metadata; onOpened: success => figures.ready(success && available && !busy) }
    SubmissionController { id: submission; onOpened: success => figures.ready(success && available && !busy) }
    MetadataController { id: freshMetadata }
    Connections {
        target: delimitation
        function onBusyChanged() {
            if (figures.opening !== 0 || delimitation.busy || figures.completed) return
            if (delimitation.preview_json !== figures.savedPreview) {
                console.error("Rejected edits/cancellation lost the cut preview"); Qt.exit(2); return
            }
            delimitation.save_cut("Cut after cancellation")
            if (delimitation.preview_json !== "[]") {
                console.error("Session was not restored after cancellation"); Qt.exit(2); return
            }
            figures.completed = true
        }
    }
    Component.onCompleted: {
        function check(value, message) { if (!value) { console.error(message); Qt.exit(2) } }
        check(genome.open_file(folder + "/sequence.fasta"), "Figure genome open failed")
        check(alignment.open_file(folder + "/alignment.afa"), "Figure alignment open failed")
        check(tree.open_file(folder + "/tree.nwk"), "Figure tree open failed")
        check(delimitation.open_file(folder + "/hypotheses.spart"), "Figure delimitation open failed")
        check(trace.open_file(folder + "/trace.ab1"), "Figure trace open failed")
        trace.cancel_calls()
        metadata.use_project(folder)
        submission.use_project(folder)
        check(submission.open(folder + "/review.submission.json"), "Submission open rejected")
        freshMetadata.use_project(folder + "/new-project")
        check(freshMetadata.create() && freshMetadata.available, "Empty metadata creation must be immediately usable")
    }
    function runFigures() {
        function check(value, message) { if (!value) { console.error(message); Qt.exit(2) } }
        check(delimitation.attach_tree(folder + "/tree.nwk", false), "Figure display tree attach failed")
        genome.resize(800, 500)
        alignment.resize(800, 500)
        tree.resize(800, 500)
        trace.resize(800, 500)
        const controllers = [genome, alignment, tree, delimitation, trace]
        for (let i = 0; i < controllers.length; i++) {
            const controller = controllers[i]
            const prefix = folder + "/figure-" + i
            controller.export_figure(prefix + ".svg", 2, true)
            controller.export_figure_png(prefix + "-1500.png", 2, true, 1500)
            controller.export_figure_png(prefix + "-320.png", 2, true, 320)
            for (const invalid of [0, -1, NaN, Infinity, 1500.5, 10001])
                controller.export_figure_png(prefix + "-protected.png", 2, true, invalid)
        }
        // Real alignment controller owns and deletes its temporary snapshots.
        const snapshot = alignment.review_snapshot()
        const original = alignment.read_sequence_file(snapshot)
        check(original.length > 0, "Alignment snapshot required")
        const retained = trace.retain_reference(folder + "/trace.ab1", snapshot)
        check(retained.length > 0 && retained !== snapshot, "Reference must transfer to shared evidence")
        check(trace.prepare_reads(JSON.stringify([folder + "/trace.ab1", folder + "/missing.ab1"]), 20), "Preparation must be accepted")
        check(alignment.review_snapshot() !== snapshot, "Snapshot replacement required")
        check(alignment.read_sequence_file(snapshot) === "", "Original snapshot must have been removed")
        check(alignment.read_sequence_file(retained) === original, "Initial request reference lost during preparation")
        trace.cancel_stage()
        check(trace.busy, "Real cancellation stays busy until terminal completion")
        check(delimitation.attach_tree(folder + "/time-tree.nwk", true), "Time tree attach failed")
        delimitation.preview_cut(0.5)
        savedPreview = delimitation.preview_json
        savedWorkspace = delimitation.workspace_json
        savedDisplay = delimitation.display_path
        check(JSON.parse(savedPreview).length > 0, "Cut preview required")
        delimitation.run_analysis("asap", folder + "/alignment.afa", false, "{}")
        check(delimitation.busy, "Registered analysis must own session")
        delimitation.save_cut("Cut during analysis")
        check(delimitation.message.indexOf("running") >= 0, "Busy cut needs useful refusal")
        check(!delimitation.attach_tree(folder + "/tree.nwk", false), "Busy tree attachment must refuse")
        check(!delimitation.save_as(folder + "/busy.spart"), "Busy save must refuse")
        delimitation.undo()
        delimitation.redo()
        delimitation.preview_cut(0.2)
        check(delimitation.preview_json === savedPreview, "Busy operations changed preview")
        check(delimitation.workspace_json === savedWorkspace, "Busy operations changed workspace")
        check(delimitation.display_path === savedDisplay, "Busy attachment changed display")
        delimitation.cancel_job()
    }
}
