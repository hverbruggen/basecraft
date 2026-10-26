import QtQuick
import QtTest

// Run the production orchestration functions with a recording controller. This
// keeps the test independent of native controllers and external fitting engines.
TestCase {
    name: "PtpSequence"
    property string orchestration: ""
    readonly property string settings: JSON.stringify({rooting: "mrttv", workers: 2, seed: 5, iqtree_model: "MFP"})

    function qmlFunction(source, name) {
        const start = source.indexOf("function " + name + "(")
        verify(start >= 0, "Production function exists: " + name)
        let end = source.indexOf("{", start) + 1
        let depth = 1
        while (depth && end < source.length) {
            const char = source[end++]
            if (char === "{") depth++
            if (char === "}") depth--
        }
        compare(depth, 0, "Complete production function: " + name)
        return source.slice(start, end)
    }
    function initTestCase() {
        const request = new XMLHttpRequest()
        request.open("GET", Qt.resolvedUrl("../../qml/DelimitationView.qml"), false)
        request.send()
        verify(request.responseText.length > 0, "Read the production QML source")
        orchestration = ["startAnalysis", "fitPtpTree", "onDisplayTreeReady"]
            .map(name => qmlFunction(request.responseText, name)).join("\n")
    }
    function workflow(options) {
        options = options || {}
        const calls = []
        const controller = {
            busy: options.busy || false, display_path: "/previous.nwk",
            attach_tree: function(path, rooted) {
                calls.push(["attach", path, rooted])
                if (options.attach !== false) this.display_path = path
                return options.attach !== false
            },
            root_display_tree: function(method) {
                calls.push(["root", method])
                if (options.root !== false) this.display_path = "/unique-rooted/tree.nwk"
                return options.root !== false
            },
            run_analysis: function(...args) { calls.push(["run", ...args]) }
        }
        // Keep QML's lexical state and doc-qualified access in the same closure.
        const make = new Function("ctl", `
            let pendingFit = null, pendingDisplay = null, showGroups = true;
            const doc = {
                get pendingFit() { return pendingFit; },
                set pendingFit(value) { pendingFit = value; },
                set showGroups(value) { showGroups = value; }
            };
            ${orchestration}
            doc.fitPtpTree = fitPtpTree;
            return {startAnalysis, onDisplayTreeReady, doc};
        `)
        return {context: make(controller), calls, controller}
    }
    function test_existing_tree_data() {
        const rows = []
        for (const method of ["ptp", "mptp"])
            for (const rooting of ["keep", "midpoint", "mrttv"])
                rows.push({tag: method + "-" + rooting, method, rooting})
        return rows
    }
    function test_existing_tree(data) {
        const w = workflow()
        w.context.startAnalysis(data.method, "/supplied.nwk", true,
            JSON.stringify(Object.assign(JSON.parse(settings), {rooting: data.rooting})), "existing")
        compare(w.calls[0], ["attach", "/supplied.nwk", false])
        if (data.rooting !== "keep") compare(w.calls[1], ["root", data.rooting])
        const fit = w.calls[w.calls.length - 1]
        compare(fit.slice(0, 4), ["run", data.method,
            data.rooting === "keep" ? "/supplied.nwk" : "/unique-rooted/tree.nwk", true])
        compare(JSON.parse(fit[4]), {workers: 2, seed: 5})
        compare(w.calls.length, data.rooting === "keep" ? 2 : 3)
    }
    function test_failure_data() {
        const rows = []
        for (const method of ["ptp", "mptp"])
            for (const failure of ["attach", "root", "busy"])
                rows.push({tag: method + "-" + failure, method, failure})
        return rows
    }
    function test_failure(data) {
        const options = {}
        options[data.failure] = data.failure === "busy"
        const w = workflow(options)
        w.context.startAnalysis(data.method, "/supplied.nwk", true, settings, "existing")
        verify(!w.calls.some(call => call[0] === "run"), "Failure prevents fitting")
    }
    function methods() { return [{tag: "ptp", method: "ptp"}, {tag: "mptp", method: "mptp"}] }
    function test_keep_requires_root_assertion_data() { return methods() }
    function test_keep_requires_root_assertion(data) {
        const w = workflow()
        w.context.startAnalysis(data.method, "/supplied.nwk", false, '{"rooting":"keep"}', "existing")
        compare(w.calls.length, 0)
    }
    function test_inference_data() {
        const rows = []
        for (const method of ["ptp", "mptp"])
            for (const success of [true, false])
                rows.push({tag: method + "-" + success, method, success})
        return rows
    }
    function test_inference(data) {
        const w = workflow()
        w.context.startAnalysis(data.method, "/input.afa", true, settings, "iqtree")
        compare(w.calls.map(call => call.slice(0, 2)), [["run", "display_iqtree"]])
        w.controller.display_path = "/inferred.nwk"
        w.context.onDisplayTreeReady(data.success)
        compare(w.context.doc.pendingFit, null)
        if (data.success) {
            compare(w.calls[1], ["root", "mrttv"])
            compare(w.calls[2].slice(0, 4), ["run", data.method, "/unique-rooted/tree.nwk", true])
            w.context.onDisplayTreeReady(true)
            compare(w.calls.length, 3, "A completion cannot fit twice")
        } else compare(w.calls.length, 1)
    }
    function test_default_midpoint_data() { return methods() }
    function test_default_midpoint(data) {
        const w = workflow()
        w.context.startAnalysis(data.method, "/input.afa", true, '{"workers":2}', "iqtree")
        w.context.onDisplayTreeReady(true)
        compare(w.calls[1], ["root", "midpoint"])
        compare(w.calls[2].slice(0, 4), ["run", data.method, "/unique-rooted/tree.nwk", true])
    }
    function test_inferred_root_failure_data() { return methods() }
    function test_inferred_root_failure(data) {
        const w = workflow({root: false})
        w.context.startAnalysis(data.method, "/input.afa", true, settings, "iqtree")
        w.context.onDisplayTreeReady(true)
        compare(w.calls.map(call => call.slice(0, 2)), [["run", "display_iqtree"], ["root", "mrttv"]])
    }
    function test_gmyc_unchanged() {
        const w = workflow()
        w.context.startAnalysis("gmyc_single", "/time.nwk", true, '{"seed":5}', "existing")
        compare(w.calls, [["run", "gmyc_single", "/time.nwk", true, '{"seed":5}']])
    }
}
