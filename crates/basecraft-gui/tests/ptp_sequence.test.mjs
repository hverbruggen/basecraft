// Exercise the actual QML orchestration functions without loading a native GUI
// or requiring external inference/fitting engines. Artifact/save tests live in
// delimitation_bridge.rs and use the real typed document operations.
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import {test} from 'node:test';

const source = readFileSync(new URL('../qml/DelimitationView.qml', import.meta.url), 'utf8');
function qmlFunction(name) {
    const start = source.indexOf(`function ${name}(`);
    assert.notEqual(start, -1);
    let end = source.indexOf('{', start) + 1;
    let depth = 1;
    while (depth && end < source.length) {
        const char = source[end++];
        if (char === '{') depth++;
        if (char === '}') depth--;
    }
    assert.equal(depth, 0);
    return source.slice(start, end);
}
function workflow({attach = true, root = true, busy = false} = {}) {
    const calls = [];
    const context = vm.createContext({
        pendingFit: null, pendingDisplay: null, showGroups: true,
        ctl: {
            busy, display_path: '/previous.nwk',
            attach_tree(path, rooted) {
                calls.push(['attach', path, rooted]);
                if (attach) this.display_path = path;
                return attach;
            },
            root_display_tree(method) {
                calls.push(['root', method]);
                if (root) this.display_path = '/unique-rooted/tree.nwk';
                return root;
            },
            run_analysis(...args) { calls.push(['run', ...args]); },
        },
    });
    context.doc = context;
    for (const name of ['startAnalysis', 'fitPtpTree', 'onDisplayTreeReady'])
        vm.runInContext(qmlFunction(name), context);
    return {context, calls};
}
const settings = JSON.stringify({rooting: 'mrttv', workers: 2, seed: 5, iqtree_model: 'MFP'});
for (const method of ['ptp', 'mptp']) {
    for (const rooting of ['keep', 'midpoint', 'mrttv']) {
        test(`${method}: existing tree ${rooting} precedes fitting`, () => {
            const {context, calls} = workflow();
            context.startAnalysis(method, '/supplied.nwk', true,
                JSON.stringify({...JSON.parse(settings), rooting}), 'existing');
            assert.deepEqual(calls[0], ['attach', '/supplied.nwk', false]);
            if (rooting !== 'keep') assert.deepEqual(calls[1], ['root', rooting]);
            const fit = calls.at(-1);
            assert.deepEqual(fit.slice(0, 4), ['run', method,
                rooting === 'keep' ? '/supplied.nwk' : '/unique-rooted/tree.nwk', true]);
            assert.deepEqual(JSON.parse(fit[4]), {workers: 2, seed: 5});
            assert.equal(calls.length, rooting === 'keep' ? 2 : 3);
        });
    }
    for (const failure of [{attach: false}, {root: false}, {busy: true}]) {
        test(`${method}: failure ${JSON.stringify(failure)} prevents fitting`, () => {
            const {context, calls} = workflow(failure);
            context.startAnalysis(method, '/supplied.nwk', true, settings, 'existing');
            assert.equal(calls.some(call => call[0] === 'run'), false);
        });
    }
    test(`${method}: preserving root requires the supplied-root assertion`, () => {
        const {context, calls} = workflow();
        context.startAnalysis(method, '/supplied.nwk', false, '{"rooting":"keep"}', 'existing');
        assert.equal(calls.length, 0);
    });
    for (const success of [true, false]) {
        test(`${method}: IQ-TREE success=${success} gates rooting and fitting`, () => {
            const {context, calls} = workflow();
            context.startAnalysis(method, '/input.afa', true, settings, 'iqtree');
            assert.deepEqual(calls.map(call => call.slice(0, 2)), [['run', 'display_iqtree']]);
            context.ctl.display_path = '/inferred.nwk';
            context.onDisplayTreeReady(success);
            assert.equal(context.pendingFit, null);
            if (success) {
                assert.deepEqual(calls[1], ['root', 'mrttv']);
                assert.deepEqual(calls[2].slice(0, 4), ['run', method, '/unique-rooted/tree.nwk', true]);
                context.onDisplayTreeReady(true);
                assert.equal(calls.length, 3, 'a completion cannot fit twice');
            } else assert.equal(calls.length, 1);
        });
    }
    test(`${method}: IQ-TREE retains midpoint as the default`, () => {
        const {context, calls} = workflow();
        context.startAnalysis(method, '/input.afa', true, '{"workers":2}', 'iqtree');
        context.onDisplayTreeReady(true);
        assert.deepEqual(calls[1], ['root', 'midpoint']);
        assert.deepEqual(calls[2].slice(0, 4), ['run', method, '/unique-rooted/tree.nwk', true]);
    });
    test(`${method}: IQ-TREE rooting/write failure prevents fitting`, () => {
        const {context, calls} = workflow({root: false});
        context.startAnalysis(method, '/input.afa', true, settings, 'iqtree');
        context.onDisplayTreeReady(true);
        assert.deepEqual(calls.map(call => call.slice(0, 2)), [['run', 'display_iqtree'], ['root', 'mrttv']]);
    });
}
test('GMYC keeps its existing route and parameters', () => {
    const {context, calls} = workflow();
    context.startAnalysis('gmyc_single', '/time.nwk', true, '{"seed":5}', 'existing');
    assert.deepEqual(calls, [['run', 'gmyc_single', '/time.nwk', true, '{"seed":5}']]);
});
