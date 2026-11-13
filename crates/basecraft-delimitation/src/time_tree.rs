//! Validated time-tree preparation and a fixed-root unweighted LS conversion.
#![allow(clippy::cast_precision_loss)]
use crate::{Error, Result};
use basecraft_docs::TreeDoc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeTree {
    /// Arena order, same as the immutable source tree.
    pub ages: Vec<f64>,
    pub root_height: f64,
    pub tips: BTreeMap<String, usize>,
    pub events: Vec<usize>,
    pub diagnostics: Vec<String>,
    /// How the tips were evened out, when they needed it.
    pub evened: Option<Evened>,
}

/// The operation that makes a tree ultrametric, offered when one is not (D17).
pub const SMOOTHING_OPERATION: &str = "basecraft.sanderson/date_tree@1";

/// How the tip branches were evened out before use (D17): every tip now sits
/// at `depth`, and no tip branch moved by more than `largest_change`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evened {
    pub depth: f64,
    pub tips_changed: usize,
    pub largest_change: f64,
}

/// Root-to-tip depths with the tips evened out, the tips by name, and how
/// they were evened.
type EvenedDepths = (Vec<f64>, BTreeMap<String, usize>, Option<Evened>);

/// The one ultrametric check (D17), behind `validate`, which the tree view
/// and every analysis use.
///
/// A tip's root-to-tip depth may differ from the true one by its bound: half
/// a unit in the last written decimal of each branch on its path, plus the
/// floating-point error of summing them (a length basecraft computed has no
/// written decimals, so only the second part). The tree is ultrametric when
/// one depth lies within every tip's bound and leaves no tip branch negative.
/// The tips are then evened out to the depth in that range that changes the
/// largest tip branch least.
fn ultrametric(tree: &TreeDoc, binary: bool) -> Result<EvenedDepths> {
    let (mut depths, tips) = depths(tree, binary)?;
    let mut bounds = vec![0.0; depths.len()];
    for i in tree.preorder() {
        let node = tree.node(i);
        if let Some(parent) = node.parent {
            let rounding = node
                .length_place
                .map_or(0.0, |place| 0.5 * 10f64.powi(place));
            bounds[i] = bounds[parent] + rounding + f64::EPSILON * depths[i];
        }
    }
    let parent_depth = |i: usize| tree.node(i).parent.map_or(0.0, |p| depths[p]);
    // The tips that set each end of the feasible range, and whether the
    // lower end comes from a tip branch that cannot go below zero.
    let (mut low, mut high) = ((f64::MIN, 0, false), (f64::MAX, 0));
    let (mut shallow, mut deep) = (f64::MAX, f64::MIN);
    for &i in tips.values() {
        let rounded = depths[i] - bounds[i];
        let floor = parent_depth(i);
        if rounded.max(floor) > low.0 {
            low = (rounded.max(floor), i, floor > rounded);
        }
        if depths[i] + bounds[i] < high.0 {
            high = (depths[i] + bounds[i], i);
        }
        shallow = shallow.min(depths[i]);
        deep = deep.max(depths[i]);
    }
    if low.0 > high.0 {
        let name = |i: usize| tree.node(i).name.clone();
        let reason = if low.2 {
            format!(
                "tip {} is at most {} from the root, but the branch to tip {} starts deeper, at {}, so evening the tips would make that branch negative",
                name(high.1),
                high.0,
                name(low.1),
                parent_depth(low.1),
            )
        } else {
            format!(
                "tip {} is {} from the root and tip {} is {}, further apart than the decimals written in the file allow",
                name(high.1),
                depths[high.1],
                name(low.1),
                depths[low.1],
            )
        };
        return Err(Error(format!(
            "the tree is not ultrametric: {reason}. Make it ultrametric with penalised likelihood (Sanderson rate smoothing, operation {SMOOTHING_OPERATION}) and use that tree"
        )));
    }
    let (low, high) = (low.0, high.0);
    let depth = f64::midpoint(shallow, deep).clamp(low, high);
    let mut evened = Evened {
        depth,
        tips_changed: 0,
        largest_change: 0.0,
    };
    for &i in tips.values() {
        let change = (depth - depths[i]).abs();
        if change > 0.0 {
            evened.tips_changed += 1;
            evened.largest_change = evened.largest_change.max(change);
        }
        depths[i] = depth;
    }
    Ok((depths, tips, (evened.tips_changed > 0).then_some(evened)))
}

/// Whether `tree` has usable lengths but is not ultrametric, so smoothing it
/// with penalised likelihood is the way on (D17). Callers decide on this
/// check, never on the refusal's text.
pub fn needs_smoothing(tree: &TreeDoc) -> bool {
    depths(tree, false).is_ok() && ultrametric(tree, false).is_err()
}

/// A rooted tree with ultrametric depths, its tips evened out (D17).
pub fn validate(tree: &TreeDoc, rooted: bool, binary: bool) -> Result<TimeTree> {
    if !rooted {
        return Err(Error("time-tree analysis requires a rooted tree".into()));
    }
    let (depths, tips, evened) = ultrametric(tree, binary)?;
    let height = evened.as_ref().map_or_else(
        || tips.values().map(|&i| depths[i]).fold(0.0, f64::max),
        |evened| evened.depth,
    );
    if height <= 0.0 || !height.is_finite() {
        return Err(Error(
            "time tree requires positive finite root height".into(),
        ));
    }
    let ages: Vec<_> = depths
        .iter()
        .map(|depth| (height - depth).max(0.0))
        .collect();
    let mut diagnostics = Vec::new();
    if let Some(evened) = &evened {
        diagnostics.push(format!(
            "tip branches evened out to a root-to-tip depth of {}: {} tips changed by at most {}, within the rounding of the written branch lengths",
            evened.depth, evened.tips_changed, evened.largest_change
        ));
    }
    for &i in tree.preorder().iter().skip(1) {
        if tree.node(i).length == Some(0.0) && !tree.node(i).is_tip() {
            // A binary encoding of a polytomy: nested events share one age and
            // the waiting interval between them is empty, as for tied clades.
            diagnostics.push(format!(
                "zero-length internal branch at node {i}; its branching events are tied (polytomy)"
            ));
        } else if tree.node(i).length.unwrap_or(0.0) / height < 1e-8 {
            diagnostics.push(format!(
                "very short branch at node {i}; recent-event rates may be weakly identified"
            ));
        }
    }
    let mut events: Vec<_> = tree.internal_preorder();
    events.sort_by(|&a, &b| ages[b].total_cmp(&ages[a]).then(a.cmp(&b)));
    Ok(TimeTree {
        ages,
        root_height: height,
        tips,
        events,
        diagnostics,
        evened,
    })
}

pub fn substitution_tips(tree: &TreeDoc, rooted: bool) -> Result<BTreeMap<String, usize>> {
    if !rooted {
        return Err(Error("explicit root or outgroup required".into()));
    }
    Ok(depths(tree, true)?.1)
}

fn depths(tree: &TreeDoc, binary: bool) -> Result<(Vec<f64>, BTreeMap<String, usize>)> {
    let mut depths = vec![0.0; tree.nodes().len()];
    let mut tips = BTreeMap::new();
    for i in tree.preorder() {
        let node = tree.node(i);
        if binary && !node.is_tip() && node.children.len() != 2 {
            return Err(Error("method requires a bifurcating tree".into()));
        }
        if let Some(parent) = node.parent {
            let length = node
                .length
                .ok_or_else(|| Error(format!("missing length at node {i}")))?;
            if !length.is_finite() || length < 0.0 {
                return Err(Error(format!(
                    "negative or nonfinite length at node {i}; resolve negative NJ limbs explicitly"
                )));
            }
            depths[i] = depths[parent] + length;
        }
        if node.is_tip() && (node.name.is_empty() || tips.insert(node.name.clone(), i).is_some()) {
            return Err(Error("empty or ambiguous tip names".into()));
        }
    }
    if tips.len() < 2 {
        return Err(Error(
            "at least two distinct tips are required; a single haplotype has no fit".into(),
        ));
    }
    Ok((depths, tips))
}

/// Cut groups use age <= threshold, including all clades tied at that age.
/// This is a manual partition, independent of GMYC likelihood fitting.
pub fn cut(tree: &TreeDoc, time: &TimeTree, threshold: f64) -> Result<Vec<BTreeSet<String>>> {
    if !threshold.is_finite() || threshold < 0.0 || threshold > time.root_height {
        return Err(Error("threshold outside time-tree axis".into()));
    }
    let mut groups = Vec::new();
    let mut stack = vec![tree.root()];
    while let Some(i) = stack.pop() {
        if tree.node(i).is_tip() || time.ages[i] <= threshold {
            let mut group = BTreeSet::new();
            let mut descendants = vec![i];
            while let Some(node) = descendants.pop() {
                if tree.node(node).is_tip() {
                    group.insert(tree.node(node).name.clone());
                } else {
                    descendants.extend(&tree.node(node).children);
                }
            }
            groups.push(group);
        } else {
            stack.extend(tree.node(i).children.iter().rev());
        }
    }
    Ok(groups)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conversion {
    pub ages: Vec<f64>,
    pub original_lengths: Vec<Option<f64>>,
    pub derived_lengths: Vec<Option<f64>>,
    pub normalization: f64,
    pub objective: String,
    pub residual_sum_squares: f64,
    pub iterations: usize,
    pub converged: bool,
}

/// Convex objective: `sum_e` (age_parent-age_child-length_e/H)^2, H = mean
/// original root-to-tip path. Root is fixed at one, all tips at zero, and
/// each parent age >= child age. This is relative branch LS, not clock dating.
/// Projected gradient uses Dykstra's halfspace projections, including fixed ages.
pub fn convert(tree: &TreeDoc, rooted: bool, max_iterations: usize) -> Result<Conversion> {
    if !rooted {
        return Err(Error(
            "relative conversion requires a fixed explicit root".into(),
        ));
    }
    let (depths, tips) = depths(tree, false)?;
    let normalization = tips.values().map(|&i| depths[i]).sum::<f64>() / tips.len() as f64;
    if normalization <= 0.0 || !normalization.is_finite() {
        return Err(Error(
            "conversion requires positive mean root-to-tip length".into(),
        ));
    }
    let mut ages: Vec<_> = depths
        .iter()
        .map(|d| (1.0 - d / normalization).clamp(0.0, 1.0))
        .collect();
    let fixed: Vec<_> = std::iter::once((tree.root(), 1.0))
        .chain(tips.values().map(|&i| (i, 0.0)))
        .collect();
    let edges: Vec<_> = tree
        .preorder()
        .iter()
        .filter_map(|&i| {
            tree.node(i)
                .parent
                .map(|p| (p, i, tree.node(i).length.unwrap() / normalization))
        })
        .collect();
    let degree = tree
        .nodes()
        .iter()
        .map(|n| n.children.len() + usize::from(n.parent.is_some()))
        .max()
        .unwrap_or(1);
    let step = 1.0 / (4.0 * degree as f64);
    project(&mut ages, &edges, &fixed)?;
    let mut converged = false;
    let mut iterations = 0;
    for iteration in 0..max_iterations {
        let mut gradient = vec![0.0; ages.len()];
        for &(p, c, length) in &edges {
            let residual = ages[p] - ages[c] - length;
            gradient[p] += 2.0 * residual;
            gradient[c] -= 2.0 * residual;
        }
        let mut next: Vec<_> = ages
            .iter()
            .zip(gradient)
            .map(|(age, grad)| age - step * grad)
            .collect();
        project(&mut next, &edges, &fixed)?;
        let change = ages
            .iter()
            .zip(&next)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        ages = next;
        iterations = iteration + 1;
        if change < 1e-12 {
            converged = true;
            break;
        }
    }
    let residual_sum_squares = edges
        .iter()
        .map(|&(p, c, length)| (ages[p] - ages[c] - length).powi(2))
        .sum();
    let original_lengths = tree.nodes().iter().map(|n| n.length).collect();
    let derived_lengths = tree
        .nodes()
        .iter()
        .enumerate()
        .map(|(i, n)| n.parent.map(|p| (ages[p] - ages[i]).max(0.0)))
        .collect();
    Ok(Conversion { ages, original_lengths,derived_lengths,normalization,objective: "sum (parent_age-child_age-original_length/mean_root_tip)^2; root=1, tips=0, parent>=child".into(),residual_sum_squares,iterations,converged })
}

fn project(ages: &mut [f64], edges: &[(usize, usize, f64)], fixed: &[(usize, f64)]) -> Result<()> {
    let mut corrections = vec![0.0; edges.len()];
    let mut fixed_corrections = vec![0.0; fixed.len()];
    for _ in 0..20_000 {
        let before = ages.to_vec();
        for (k, &(p, c, _)) in edges.iter().enumerate() {
            let parent = ages[p] + corrections[k];
            let child = ages[c] - corrections[k];
            let adjustment = ((child - parent) / 2.0).max(0.0);
            ages[p] = parent + adjustment;
            ages[c] = child - adjustment;
            corrections[k] = -adjustment;
        }
        for (k, &(i, value)) in fixed.iter().enumerate() {
            let updated = ages[i] + fixed_corrections[k];
            fixed_corrections[k] = updated - value;
            ages[i] = value;
        }
        if ages.iter().zip(before).all(|(a, b)| (a - b).abs() < 1e-13)
            && edges.iter().all(|&(p, c, _)| ages[p] + 1e-12 >= ages[c])
        {
            return Ok(());
        }
    }
    Err(Error(
        "relative-time constraint projection did not converge".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clocklike_conversion_and_scaling() {
        for scale in [0.001, 1.0, 1000.0] {
            let tree = TreeDoc::from_newick(
                "test",
                &format!(
                    "((a:{},b:{}):{},(c:{},d:{}):{});",
                    scale,
                    scale,
                    scale,
                    scale * 0.5,
                    scale * 0.5,
                    scale * 1.5
                ),
            )
            .unwrap();
            let time = validate(&tree, true, true).unwrap();
            assert_eq!(cut(&tree, &time, scale).unwrap().len(), 2);
            let result = convert(&tree, true, 10_000).unwrap();
            assert!(result.converged);
            assert!(result.residual_sum_squares < 1e-20);
            assert_eq!(result.ages[0], 1.0);
        }
    }
    #[test]
    fn invalid_time_trees_are_rejected() {
        for newick in [
            "(a:1.0,b:2.0);",
            "(a:-1,b:1);",
            "(a:0,b:0);",
            "(a:1,a:1);",
            "(a:1,b:1,c:1);",
        ] {
            let tree = TreeDoc::from_newick("bad", newick).unwrap();
            assert!(validate(&tree, true, true).is_err(), "{newick}");
        }
        let tree = TreeDoc::from_newick("good", "(a:1,b:1);").unwrap();
        assert!(validate(&tree, false, true).is_err());
    }
    #[test]
    fn six_decimal_trees_pass_within_their_rounding_and_tips_are_evened_out() {
        // As BEAST and TreeAnnotator write it: six decimals, so a and b differ
        // by one unit in the last place.
        let tree = TreeDoc::from_newick(
            "beast",
            "((a:0.123457,b:0.123456):0.876543,(c:0.333333,d:0.333333):0.666667);",
        )
        .unwrap();
        let time = validate(&tree, true, true).unwrap();
        let evened = time.evened.clone().unwrap();
        assert!((evened.depth - 0.999_999_5).abs() < 1e-12, "{evened:?}");
        assert!(evened.largest_change <= 5e-7 + 1e-12, "{evened:?}");
        assert_eq!(time.root_height, evened.depth);
        assert!(time.tips.values().all(|&tip| time.ages[tip] == 0.0));
        assert!(
            time.diagnostics
                .iter()
                .any(|d| d.starts_with("tip branches evened out"))
        );
        // An exactly ultrametric tree needs no evening.
        let exact =
            TreeDoc::from_newick("exact", "((a:0.5,b:0.5):0.5,(c:0.25,d:0.25):0.75);").unwrap();
        assert!(validate(&exact, true, true).unwrap().evened.is_none());
    }

    #[test]
    fn a_tree_off_by_more_than_its_rounding_is_refused_with_the_smoothing_offer() {
        // a and b differ by 4 units in the sixth decimal; their paths allow 2.
        let tree = TreeDoc::from_newick(
            "off",
            "((a:0.123460,b:0.123456):0.876543,(c:0.333333,d:0.333333):0.666667);",
        )
        .unwrap();
        let error = validate(&tree, true, true).unwrap_err().0;
        assert!(
            error.contains("not ultrametric") && error.contains(SMOOTHING_OPERATION),
            "{error}"
        );
        assert!(
            error.contains("tip b") && error.contains("tip a"),
            "{error}"
        );
        // A length written without a decimal point is exact (D17 refined),
        // so whole-number trees get no rounding allowance.
        for whole in ["(a:1,b:2);", "((a:3,b:1):1,(c:2,d:2):2);"] {
            let tree = TreeDoc::from_newick("whole", whole).unwrap();
            assert!(validate(&tree, true, true).is_err(), "{whole}");
        }
    }

    #[test]
    fn the_refusal_names_the_tips_in_conflict_and_a_branch_that_would_go_negative() {
        // a sits deeper than b can reach; c, written to one decimal, is loose
        // enough to fit either, so the refusal names a and b, not c.
        let tree = TreeDoc::from_newick("mixed", "((a:0.500,b:0.400):0.500,c:1.0);").unwrap();
        let error = validate(&tree, true, false).unwrap_err().0;
        assert!(
            error.contains("tip b") && error.contains("tip a") && !error.contains("tip c"),
            "{error}"
        );
        // b's branch starts at 0.9, deeper than c can reach, so evening the
        // tips would make it negative.
        let tree = TreeDoc::from_newick(
            "negative",
            "((a:0.0001,(b:0.0001):0.9000):0.0000,c:0.0001);",
        )
        .unwrap();
        let error = validate(&tree, true, false).unwrap_err().0;
        assert!(error.contains("negative"), "{error}");
    }

    #[test]
    fn exact_and_split_lengths_keep_their_precision() {
        // Moving the root of an exact branch: exact pieces, floating-point
        // bound only.
        let mut tree = TreeDoc::from_newick("moved", "(a:1,b:1);").unwrap();
        tree.set_root_position(&["a".into()], 0.4).unwrap();
        assert!(validate(&tree, true, true).is_err());
        tree.set_root_position(&["a".into()], 0.5).unwrap();
        assert!(validate(&tree, true, true).unwrap().evened.is_none());
        // A six-decimal tree still passes after midpoint rooting: the pieces
        // of the split branch keep its six decimals (D17 refined).
        let six = "((a:0.123457,b:0.123456):0.876543,(c:0.333333,d:0.333333):0.666667);";
        let mut tree = TreeDoc::from_newick("beast", six).unwrap();
        tree.midpoint_root().unwrap();
        assert!(
            tree.nodes()
                .iter()
                .skip(1)
                .all(|n| n.length_place == Some(-6))
        );
        assert!(validate(&tree, true, true).is_ok());
    }

    #[test]
    fn zero_length_internal_branch_is_a_tied_nested_event() {
        let tree = TreeDoc::from_newick("polytomy", "(((a:1,b:1):0,c:1):1,d:2);").unwrap();
        let time = validate(&tree, true, true).unwrap();
        let ages: Vec<_> = time.events.iter().map(|&i| time.ages[i]).collect();
        assert_eq!(ages, [2.0, 1.0, 1.0]);
        assert!(
            time.diagnostics
                .iter()
                .any(|d| d.contains("tied (polytomy)"))
        );
        // Both tied nodes join one cut group; the order of resolution is irrelevant.
        assert_eq!(cut(&tree, &time, 1.0).unwrap().len(), 2);
    }
    #[test]
    fn tied_separate_events_and_constrained_nonclocklike_tree() {
        let tree = TreeDoc::from_newick("tied", "((a:1,b:1):1,(c:1,d:1):1);").unwrap();
        let time = validate(&tree, true, true).unwrap();
        assert_eq!(cut(&tree, &time, 1.0).unwrap().len(), 2);
        assert_eq!(cut(&tree, &time, 0.9).unwrap().len(), 4);
        let nonclock =
            TreeDoc::from_newick("nonclock", "((a:10,b:0.1):0.01,(c:1,d:2):0.1);").unwrap();
        let result = convert(&nonclock, true, 10_000).unwrap();
        assert!(result.converged);
        for (i, node) in nonclock.nodes().iter().enumerate() {
            if let Some(parent) = node.parent {
                assert!(result.ages[parent] + 1e-10 >= result.ages[i]);
            }
            if node.is_tip() {
                assert_eq!(result.ages[i], 0.0);
            }
        }
    }
}
