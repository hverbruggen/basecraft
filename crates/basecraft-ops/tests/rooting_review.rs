//! Independent REV-0003 probes against the automatic-rooting public API.
use basecraft_docs::TreeDoc;
use basecraft_ops::{DocId, Session, files};
use basecraft_protocol::Op;

fn methods() -> [Op; 2] {
    [Op::TreeMidpointRoot {}, Op::TreeMinimumVarianceRoot {}]
}

#[test]
fn supported_basal_branch_survives_automatic_root_save_reload() {
    let mut failures = Vec::new();
    for op in methods() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("supported.nwk");
        std::fs::write(&path, "(A:1,(B:10,C:1)90:1);").unwrap();
        let mut session = Session::new(dir.path());
        let id = session.open(&path).unwrap();
        let before = session.tree(id).unwrap().to_newick();
        if session.apply(id, op.clone()).is_err() {
            // Explicit nonmutating refusal is preferable to metadata loss.
            assert_eq!(session.tree(id).unwrap().to_newick(), before);
            assert!(!session.can_undo(id).unwrap());
            continue;
        }
        let rooted = session.tree(id).unwrap().clone();
        assert!(
            rooted
                .supports()
                .iter()
                .any(|s| s.values.iter().flatten().any(|v| v == "90"))
        );
        session.save(id, None).unwrap();
        let loaded = files::load_tree(&path).unwrap().doc;
        if loaded.supports() != rooted.supports() {
            failures.push(format!(
                "{} saved {}: before {:?}; reloaded {:?}",
                op.name(),
                rooted.to_newick(),
                rooted.supports(),
                loaded.supports()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn automatic_root_invalid_document_returns_error_without_panic() {
    let mut failures = Vec::new();
    for op in methods() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new(dir.path());
        // Existing operation establishes the API's unknown-document contract.
        assert!(
            session
                .apply(
                    DocId(0),
                    Op::TreeReroot {
                        clade: vec!["A".into()]
                    }
                )
                .is_err()
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.apply(DocId(0), op.clone())
        }));
        match result {
            Ok(result) => assert!(result.is_err()),
            Err(_) => failures.push(op.name()),
        }
    }
    assert!(
        failures.is_empty(),
        "unknown document panicked for {failures:?}"
    );
}

#[test]
fn refusal_in_recoverable_session_creates_no_tracking_or_history() {
    for text in [
        "(A,B:1);",
        "(A:0,B:0);",
        "(A:1,A:2);",
        "(A:-1,B:2);",
        // Reversing the supported edge would overwrite the named old root.
        "((A:10,B:1)90:1,C:1)named;",
    ] {
        for op in methods() {
            let dir = tempfile::tempdir().unwrap();
            let paths = basecraft_platform::paths::AppPaths::from_environment(|key| {
                (key == "BASECRAFT_PROFILE_DIR")
                    .then(|| dir.path().join("profile").into_os_string())
            })
            .unwrap();
            let path = dir.path().join("invalid.nwk");
            std::fs::write(&path, text).unwrap();
            let mut session = Session::recoverable_with_paths(dir.path(), &paths).unwrap();
            let id = session.open(&path).unwrap();
            let before = session.tree(id).unwrap().to_newick();
            let revision = session.record_revision(id).unwrap();
            assert!(!session.recovery_root().exists());
            assert!(session.apply(id, op).is_err());
            assert_eq!(session.tree(id).unwrap().to_newick(), before);
            assert_eq!(session.record_revision(id).unwrap(), revision);
            assert!(!session.can_undo(id).unwrap());
            assert!(!session.can_redo(id).unwrap());
            assert!(!session.recovery_root().exists());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
            assert!(!files::tree_sidecar(&path).exists());
        }
    }
}

fn distances(doc: &TreeDoc, start: usize) -> Vec<f64> {
    let mut values = vec![0.0; doc.nodes().len()];
    let mut stack = vec![(start, usize::MAX)];
    while let Some((node, previous)) = stack.pop() {
        let mut neighbours: Vec<_> = doc
            .node(node)
            .children
            .iter()
            .map(|&c| (c, doc.node(c).length.unwrap()))
            .collect();
        if let Some(parent) = doc.node(node).parent {
            neighbours.push((parent, doc.node(node).length.unwrap()));
        }
        for (next, length) in neighbours {
            if next != previous {
                values[next] = values[node] + length;
                stack.push((next, node));
            }
        }
    }
    values
}

// All inputs are small integer lengths. Fit integer variance coefficients
// directly from d_i(x) = d_i(0) + s_i*x; no production moment recurrence or
// three-point floating quadratic interpolation is reused.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn integer_oracle(doc: &TreeDoc) -> f64 {
    let tips = doc.tips_in_order();
    let n = i64::try_from(tips.len()).unwrap();
    let mut best = f64::INFINITY;
    for (child, node) in doc.nodes().iter().enumerate() {
        let Some(parent) = node.parent else { continue };
        let d = distances(doc, parent);
        let mut below = vec![false; doc.nodes().len()];
        let mut stack = vec![child];
        while let Some(i) = stack.pop() {
            below[i] = true;
            stack.extend(&doc.node(i).children);
        }
        let mut sum_d = 0;
        let mut sum_s = 0;
        let mut sum_sd = 0;
        let mut sum_dd = 0;
        for &tip in &tips {
            let distance = d[tip] as i64;
            let sign = if below[tip] { -1 } else { 1 };
            sum_d += distance;
            sum_s += sign;
            sum_sd += sign * distance;
            sum_dd += distance * distance;
        }
        let quadratic = n * n - sum_s * sum_s;
        let linear = 2 * (n * sum_sd - sum_s * sum_d);
        let constant = n * sum_dd - sum_d * sum_d;
        let x = (-(linear as f64) / (2 * quadratic) as f64).clamp(0.0, node.length.unwrap());
        let value = ((quadratic as f64 * x + linear as f64) * x + constant as f64) / (n * n) as f64;
        best = best.min(value);
    }
    best
}

fn next(state: &mut u64, range: usize) -> usize {
    *state = *state * 48_271 % 2_147_483_647;
    usize::try_from(*state % u64::try_from(range).unwrap()).unwrap()
}

fn random_tree(seed: u64) -> TreeDoc {
    let mut state = seed;
    let mut components: Vec<_> = (0..3 + next(&mut state, 10))
        .map(|i| format!("T{i}"))
        .collect();
    while components.len() > 1 {
        let count = (2 + next(&mut state, 2)).min(components.len());
        let children: Vec<_> = (0..count)
            .map(|_| {
                let i = next(&mut state, components.len());
                format!("{}:{}", components.swap_remove(i), 1 + next(&mut state, 20))
            })
            .collect();
        components.push(format!("({})", children.join(",")));
    }
    TreeDoc::from_newick("oracle", &format!("{};", components[0])).unwrap()
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn diverse_topologies_match_integer_variance_and_diameter_oracles() {
    for seed in 1..=100 {
        let original = random_tree(seed);
        let tips = original.tips_in_order();
        let diameter = tips
            .iter()
            .flat_map(|&a| {
                let d = distances(&original, a);
                tips.iter().map(move |&b| d[b])
            })
            .fold(0.0_f64, f64::max);
        let expected = integer_oracle(&original);
        for method in [TreeDoc::midpoint_root, TreeDoc::minimum_variance_root] {
            let mut rooted = original.clone();
            method(&mut rooted).unwrap();
            let reference = distances(&rooted, rooted.root());
            // Reverse child order and change the old root without changing
            // the unrooted tree. Compare metric roots by every tip distance.
            let mut reordered = original.clone();
            reordered
                .rotate(&reordered.clade_ref(reordered.root()))
                .unwrap();
            let mut rerooted = original.clone();
            rerooted
                .reroot(&[original.node(tips[0]).name.clone()])
                .unwrap();
            for mut variant in [reordered, rerooted] {
                method(&mut variant).unwrap();
                let actual = distances(&variant, variant.root());
                for &tip in &tips {
                    let name = &original.node(tip).name;
                    let a = rooted.find_tip(name).unwrap();
                    let b = variant.find_tip(name).unwrap();
                    assert!(
                        (reference[a] - actual[b]).abs() < 1e-9,
                        "root/order changed at seed {seed}"
                    );
                }
            }
            for &tip in &tips {
                let old = distances(&original, tip);
                let new = distances(&rooted, rooted.find_tip(&original.node(tip).name).unwrap());
                for &other in &tips {
                    let i = rooted.find_tip(&original.node(other).name).unwrap();
                    assert!((old[other] - new[i]).abs() < 1e-10, "seed {seed}");
                }
            }
        }
        let mut midpoint = original.clone();
        midpoint.midpoint_root().unwrap();
        let d = distances(&midpoint, midpoint.root());
        let max = midpoint
            .tips_in_order()
            .iter()
            .map(|&t| d[t])
            .fold(0.0_f64, f64::max);
        assert!((max - diameter / 2.0).abs() < 1e-10, "seed {seed}");
        let mut variance = original.clone();
        variance.minimum_variance_root().unwrap();
        let d = distances(&variance, variance.root());
        let depths: Vec<_> = variance.tips_in_order().iter().map(|&t| d[t]).collect();
        let mean = depths.iter().sum::<f64>() / depths.len() as f64;
        let actual = depths.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / depths.len() as f64;
        assert!(
            (actual - expected).abs() < 1e-10,
            "seed {seed}: {actual} != {expected}"
        );
    }
}

#[test]
fn iterative_newick_writer_preserves_names_unary_nodes_and_missing_supports() {
    for text in [
        "(('A B':1,'C,D':2)'inner name':3,E:4)named;",
        "((A:1)unary:2,B:3,C:4);",
        "((A:1,B:2)90/-:3,(C:4,D:5)-/0.8:6);",
        "(('O''Brien':0,'µ':1)label:2,Z:3);",
        "Single;",
    ] {
        let original = TreeDoc::from_newick("serialization", text).unwrap();
        let saved = original.to_newick();
        let reloaded = TreeDoc::from_newick("serialization", &saved).unwrap();
        assert_eq!(original.nodes(), reloaded.nodes(), "{text} -> {saved}");
        assert_eq!(
            original.supports(),
            reloaded.supports(),
            "{text} -> {saved}"
        );
    }
}
