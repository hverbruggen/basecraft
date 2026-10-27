Item {
    property string folder: FIGURE_TEST_FOLDER
    GenomeController { id: genome }
    AlignmentController { id: alignment }
    TreeController { id: tree }
    DelimitationController { id: delimitation }
    SangerController { id: trace }
    Component.onCompleted: {
        function check(value, message) { if (!value) { console.error(message); Qt.exit(2) } }
        check(genome.open_file(folder + "/sequence.fasta"), "Figure genome open failed")
        check(alignment.open_file(folder + "/alignment.afa"), "Figure alignment open failed")
        check(tree.open_file(folder + "/tree.nwk"), "Figure tree open failed")
        check(delimitation.open_file(folder + "/hypotheses.spart"), "Figure delimitation open failed")
        check(delimitation.attach_tree(folder + "/tree.nwk", false), "Figure display tree attach failed")
        check(trace.open_file(folder + "/trace.ab1"), "Figure trace open failed")
        trace.cancel_calls()
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
    }
}
