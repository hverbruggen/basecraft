//! Established mixed waiting-time likelihood with pooled plug-in rates.
//! This module contains no rate regularization, prior integration or time jitter.
#![allow(
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::manual_midpoint
)]
use crate::{Error, Result, time_tree::TimeTree};
use basecraft_docs::TreeDoc;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fit {
    pub roots: Vec<usize>,
    pub threshold: Option<f64>,
    pub thresholds: Vec<f64>,
    pub log_likelihood: f64,
    pub log_lambda_coal: Option<f64>,
    pub log_lambda_div: f64,
    pub p_coal: Option<f64>,
    pub p_div: f64,
    pub log_exposure_coal: Option<f64>,
    pub log_exposure_div: f64,
    pub entities: usize,
    pub nonsingleton_clusters: usize,
    pub evaluations: usize,
    pub converged: bool,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GmycResult {
    pub method: String,
    pub profile: Vec<Fit>,
    pub best: usize,
    pub likelihood_ratio: f64,
    pub lr_degrees_of_freedom: usize,
    pub reference: String,
    pub evaluated_candidates: usize,
    pub global_optimum: bool,
    pub stopping_condition: String,
    pub starts: Vec<f64>,
    pub warnings: Vec<String>,
}

/// Sparse multiplicities of coalescent lineage counts for each chronological event.
/// Powers are cached once per distinct count at each objective evaluation.
struct Prepared {
    bases: Vec<f64>,
    coal: Vec<Vec<(usize, usize)>>,
    div: Vec<f64>,
    dt: Vec<f64>,
    coal_events: usize,
    div_events: usize,
    root_height: f64,
}

fn prepare(tree: &TreeDoc, time: &TimeTree, roots: &[usize]) -> Result<Prepared> {
    let mut owner = vec![None; tree.nodes().len()];
    let root_set: BTreeSet<_> = roots.iter().copied().collect();
    if root_set.len() != roots.len() || roots.is_empty() {
        return Err(Error(
            "mixed likelihood requires distinct nonsingleton clusters".into(),
        ));
    }
    for (group, &root) in roots.iter().enumerate() {
        if root >= tree.nodes().len() || tree.node(root).is_tip() || root == tree.root() {
            return Err(Error("invalid mixed-likelihood cluster root".into()));
        }
        let mut stack = vec![root];
        while let Some(i) = stack.pop() {
            if owner[i].replace(group).is_some() {
                return Err(Error("nested or overlapping cluster roots".into()));
            }
            stack.extend(&tree.node(i).children);
        }
    }
    let mut counts = vec![0usize; roots.len()];
    let mut histogram = BTreeMap::from([(0usize, roots.len())]);
    let mut raw = Vec::new();
    let mut div = Vec::new();
    let mut dt = Vec::new();
    let mut diversification = 1usize;
    let mut coal_events = 0;
    for (event, &node) in time.events.iter().enumerate() {
        if let Some(group) = owner[node] {
            coal_events += 1;
            let old = counts[group] * (counts[group].saturating_sub(1));
            if let Some(count) = histogram.get_mut(&old) {
                *count -= 1;
                if *count == 0 {
                    histogram.remove(&old);
                }
            }
            if root_set.contains(&node) {
                counts[group] = 2;
                diversification = diversification
                    .checked_sub(1)
                    .ok_or_else(|| Error("invalid diversification lineage state".into()))?;
            } else {
                counts[group] += 1;
            }
            *histogram
                .entry(counts[group] * (counts[group] - 1))
                .or_default() += 1;
        } else {
            diversification += 1;
        }
        div.push(diversification as f64);
        raw.push(histogram.clone());
        let next = time.events.get(event + 1).map_or(0.0, |&i| time.ages[i]);
        dt.push((time.ages[node] - next) / time.root_height);
    }
    let values: BTreeSet<_> = raw.iter().flat_map(|h| h.keys().copied()).collect();
    let indexes: BTreeMap<_, _> = values
        .iter()
        .enumerate()
        .map(|(i, &base)| (base, i))
        .collect();
    let coal = raw
        .into_iter()
        .map(|h| {
            h.into_iter()
                .map(|(base, count)| (indexes[&base], count))
                .collect()
        })
        .collect();
    let bases = values.into_iter().map(|b| b as f64).collect();
    Ok(Prepared {
        bases,
        coal,
        div,
        dt,
        coal_events,
        div_events: time.events.len() - coal_events,
        root_height: time.root_height,
    })
}

fn log_power(base: f64, exponent: f64) -> f64 {
    if base == 0.0 {
        if exponent > 0.0 {
            f64::NEG_INFINITY
        } else if exponent == 0.0 {
            0.0
        } else {
            f64::INFINITY
        }
    } else {
        base.ln() * exponent
    }
}
fn logsum(values: impl IntoIterator<Item = f64>) -> f64 {
    values.into_iter().fold(f64::NEG_INFINITY, logadd)
}
fn logadd(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY {
        return b;
    }
    if b == f64::NEG_INFINITY {
        return a;
    }
    let m = a.max(b);
    if !m.is_finite() {
        return m;
    }
    m + ((a - m).exp() + (b - m).exp()).ln()
}

struct Likelihood {
    value: f64,
    coal_rate: f64,
    div_rate: f64,
    coal_exposure: f64,
    div_exposure: f64,
}
impl Prepared {
    fn likelihood(&self, q: [f64; 2]) -> Option<Likelihood> {
        let powers: Vec<_> = self.bases.iter().map(|&b| log_power(b, q[0])).collect();
        let coal: Vec<_> = self
            .coal
            .iter()
            .map(|hist| {
                logsum(
                    hist.iter()
                        .map(|&(i, count)| powers[i] + (count as f64).ln()),
                )
            })
            .collect();
        let div: Vec<_> = self.div.iter().map(|&b| log_power(b, q[1])).collect();
        let exposure = |hazards: &[f64]| {
            logsum(
                hazards
                    .iter()
                    .zip(&self.dt)
                    .filter(|(_, dt)| **dt > 0.0)
                    .map(|(h, dt)| h + dt.ln()),
            )
        };
        let coal_exposure = exposure(&coal);
        let div_exposure = exposure(&div);
        if !coal_exposure.is_finite()
            || !div_exposure.is_finite()
            || self.coal_events == 0
            || self.div_events == 0
        {
            return None;
        }
        let coal_rate = (self.coal_events as f64).ln() - coal_exposure;
        let div_rate = (self.div_events as f64).ln() - div_exposure;
        // Pooled plug-in rates make the integrated hazard exactly the event count.
        let value = coal
            .iter()
            .zip(&div)
            .map(|(c, d)| logadd(c + coal_rate, d + div_rate))
            .sum::<f64>()
            - self.dt.len() as f64;
        value.is_finite().then_some(Likelihood {
            value,
            coal_rate,
            div_rate,
            coal_exposure,
            div_exposure,
        })
    }
}

/// R optim's two-dimensional simplex conventions: start=(1,1), step=0.1,
/// reflection=1, contraction=0.5, expansion=2, fixed initial relative tolerance.
fn optimize(
    f: impl Fn([f64; 2]) -> f64,
    start: [f64; 2],
    budget: usize,
) -> ([f64; 2], usize, bool) {
    let bounded = |q| {
        let value = f(q);
        if value.is_finite() { value } else { 1e35 }
    };
    let mut points = [start, start, start];
    let step = (0.1 * start[0].abs().max(start[1].abs())).max(0.1);
    points[1][0] += step;
    points[2][1] += step;
    let initial = bounded(start);
    let tolerance = f64::EPSILON.sqrt() * (initial.abs() + f64::EPSILON.sqrt());
    let mut values = [initial, bounded(points[1]), bounded(points[2])];
    let mut evaluations = 3;
    let mut low = 0;
    let mut converged = false;
    while evaluations <= budget {
        let mut high = low;
        let mut lowest = values[low];
        let mut highest = lowest;
        for (j, &value) in values.iter().enumerate() {
            if value < lowest {
                low = j;
                lowest = value;
            }
            if value > highest {
                high = j;
                highest = value;
            }
        }
        if highest <= lowest + tolerance {
            converged = true;
            break;
        }
        let center = [
            (points.iter().map(|p| p[0]).sum::<f64>() - points[high][0]) / 2.0,
            (points.iter().map(|p| p[1]).sum::<f64>() - points[high][1]) / 2.0,
        ];
        let reflected = [
            2.0 * center[0] - points[high][0],
            2.0 * center[1] - points[high][1],
        ];
        let reflection = bounded(reflected);
        evaluations += 1;
        if reflection < lowest {
            let expanded = [
                2.0 * reflected[0] - center[0],
                2.0 * reflected[1] - center[1],
            ];
            let expansion = bounded(expanded);
            evaluations += 1;
            if expansion < reflection {
                points[high] = expanded;
                values[high] = expansion;
            } else {
                points[high] = reflected;
                values[high] = reflection;
            }
        } else {
            if reflection < highest {
                points[high] = reflected;
                values[high] = reflection;
            }
            let contracted = [
                (points[high][0] + center[0]) / 2.0,
                (points[high][1] + center[1]) / 2.0,
            ];
            let contraction = bounded(contracted);
            evaluations += 1;
            if contraction < values[high] {
                points[high] = contracted;
                values[high] = contraction;
            } else if reflection >= highest {
                for j in 0..3 {
                    if j != low {
                        points[j] = [
                            (points[j][0] + points[low][0]) / 2.0,
                            (points[j][1] + points[low][1]) / 2.0,
                        ];
                        values[j] = bounded(points[j]);
                        evaluations += 1;
                    }
                }
            }
        }
    }
    // Return the best evaluated vertex even when the iteration budget is reached.
    let low = (0..3)
        .min_by(|&a, &b| values[a].total_cmp(&values[b]))
        .unwrap();
    (points[low], evaluations, converged)
}

pub fn evaluate(
    tree: &TreeDoc,
    time: &TimeTree,
    roots: &[usize],
    threshold: Option<f64>,
) -> Result<Fit> {
    let prepared = prepare(tree, time, roots)?;
    let (q, evaluations, converged) = optimize(
        |q| prepared.likelihood(q).map_or(f64::INFINITY, |l| -l.value),
        [1.0, 1.0],
        500,
    );
    let l = prepared
        .likelihood(q)
        .ok_or_else(|| Error("unidentified GMYC rate or nonfinite likelihood".into()))?;
    let mut diagnostics = Vec::new();
    if !converged {
        diagnostics.push("reference-start optimizer reached its evaluation budget".into());
    }
    if l.coal_exposure < (1e-10f64).ln() || l.div_exposure < (1e-10f64).ln() {
        diagnostics.push("tiny effective exposure; plug-in rate is weakly identified".into());
    }
    if q.iter().any(|p| p.abs() > 10.0 || p.abs() < 1e-4) {
        diagnostics.push("extreme or boundary exponent; inspect the likelihood profile".into());
    }
    // Multiple starts are diagnostic, never silently substituted for the reference start.
    for start in [[0.5, 0.5], [2.0, 2.0]] {
        let (other, _, _) = optimize(
            |q| prepared.likelihood(q).map_or(f64::INFINITY, |l| -l.value),
            start,
            500,
        );
        if prepared
            .likelihood(other)
            .is_some_and(|check| check.value > l.value + 1e-5)
        {
            diagnostics.push(format!(
                "alternate start {start:?} improves log likelihood; reference-start local optimum"
            ));
        }
    }
    Ok(Fit {
        roots: roots.to_vec(),
        threshold,
        thresholds: reduce_thresholds(tree, time, roots),
        log_likelihood: l.value - time.events.len() as f64 * prepared.root_height.ln(),
        log_lambda_coal: Some(l.coal_rate - prepared.root_height.ln()),
        log_lambda_div: l.div_rate - prepared.root_height.ln(),
        p_coal: Some(q[0]),
        p_div: q[1],
        log_exposure_coal: Some(l.coal_exposure + prepared.root_height.ln()),
        log_exposure_div: l.div_exposure + prepared.root_height.ln(),
        entities: prepared.div_events + 1,
        nonsingleton_clusters: roots.len(),
        evaluations,
        converged,
        diagnostics,
    })
}

fn null(time: &TimeTree) -> Fit {
    let dt: Vec<_> = time
        .events
        .iter()
        .enumerate()
        .map(|(i, &node)| {
            (time.ages[node] - time.events.get(i + 1).map_or(0.0, |&n| time.ages[n]))
                / time.root_height
        })
        .collect();
    let bases: Vec<_> = (2..=time.events.len() + 1)
        .map(|k| ((k * (k - 1)) as f64).ln())
        .collect();
    let value = |p: f64| {
        let exposure = logsum(
            bases
                .iter()
                .zip(&dt)
                .filter(|(_, t)| **t > 0.0)
                .map(|(base, t)| p * base + t.ln()),
        );
        let rate = (dt.len() as f64).ln() - exposure;
        (
            bases.iter().map(|base| rate + p * base).sum::<f64>() - dt.len() as f64,
            rate,
            exposure,
        )
    };
    let (mut left, mut right) = (0.0, 5.0);
    let ratio = (5.0f64.sqrt() - 1.0) / 2.0;
    let mut a = right - ratio * (right - left);
    let mut b = left + ratio * (right - left);
    let mut evaluations = 0;
    while right - left > 1e-8 {
        if value(a).0 > value(b).0 {
            right = b;
            b = a;
            a = right - ratio * (right - left);
        } else {
            left = a;
            a = b;
            b = left + ratio * (right - left);
        }
        evaluations += 1;
    }
    let p = (left + right) / 2.0;
    let (likelihood, rate, exposure) = value(p);
    Fit { roots: vec![],threshold:None,thresholds:vec![time.root_height],log_likelihood: likelihood-dt.len() as f64*time.root_height.ln(),log_lambda_coal:None,log_lambda_div:rate-time.root_height.ln(),p_coal:None,p_div:p,log_exposure_coal:None,log_exposure_div:exposure+time.root_height.ln(),entities:1,nonsingleton_clusters:usize::from(time.tips.len()>1),evaluations,converged:true,diagnostics:vec!["null is a single generalized coalescent population; lambda.div is the reference output field name".into()] }
}
fn roots_at(tree: &TreeDoc, time: &TimeTree, threshold: f64) -> Vec<usize> {
    tree.internal_preorder()
        .into_iter()
        .filter(|&i| {
            i != tree.root()
                && time.ages[i] <= threshold
                && tree
                    .node(i)
                    .parent
                    .is_some_and(|p| time.ages[p] > threshold)
        })
        .collect()
}
fn cancelled(cancel: Option<&Path>) -> bool {
    cancel.is_some_and(Path::exists)
}

pub fn single(tree: &TreeDoc, time: &TimeTree, cancel: Option<&Path>) -> Result<GmycResult> {
    if time.ages.iter().any(|age| !age.is_finite())
        || time.events.iter().any(|&i| time.ages[i] <= 0.0)
    {
        return Err(Error(
            "GMYC requires positive event ages; zero-time events are not jittered".into(),
        ));
    }
    let mut profile = vec![null(time)];
    let mut seen = BTreeSet::new();
    let candidates: Vec<_> = time
        .events
        .iter()
        .skip(1)
        .filter_map(|&i| {
            let roots = roots_at(tree, time, time.ages[i]);
            seen.insert(roots.clone()).then_some((roots, time.ages[i]))
        })
        .collect();
    let fits: Vec<_> = candidates
        .par_iter()
        .map(|(roots, threshold)| {
            if cancelled(cancel) {
                return Err(Error("cancelled between GMYC candidates".into()));
            }
            evaluate(tree, time, roots, Some(*threshold))
        })
        .collect();
    for fit in fits {
        profile.push(fit?);
    }
    if cancelled(cancel) {
        return Err(Error("cancelled after GMYC candidates".into()));
    }
    Ok(result(
        "single",
        profile,
        true,
        "all admissible distinct threshold partitions evaluated".into(),
        vec![],
    ))
}
fn result(
    method: &str,
    profile: Vec<Fit>,
    global_optimum: bool,
    stopping_condition: String,
    starts: Vec<f64>,
) -> GmycResult {
    let best = (0..profile.len())
        .max_by(|&a, &b| {
            profile[a]
                .log_likelihood
                .total_cmp(&profile[b].log_likelihood)
                .then(b.cmp(&a))
        })
        .unwrap_or(0);
    let likelihood_ratio = 2.0 * (profile[best].log_likelihood - profile[0].log_likelihood);
    let evaluated_candidates = profile.len();
    GmycResult { method:method.into(),profile,best,likelihood_ratio,lr_degrees_of_freedom:2,reference:"splits plug-in pooled-rate waiting-time convention; reference numerical acceptance pending".into(),evaluated_candidates,global_optimum,stopping_condition,starts,warnings:vec!["Convergence does not establish statistical support or biological correctness".into(),"Recent-age clamp in splits is not applied; compatibility comparisons must record it explicitly".into()] }
}

/// The reference split/fuse neighborhood excludes the root and all-singleton
/// proposals. Tip siblings permit fusion; split keeps nonsingleton descendants.
fn neighbors(tree: &TreeDoc, roots: &[usize]) -> BTreeSet<Vec<usize>> {
    let mut out = BTreeSet::new();
    if roots.len() <= 1 {
        return out;
    }
    for &root in roots {
        let mut split: Vec<_> = roots
            .iter()
            .copied()
            .filter(|&i| i != root)
            .chain(
                tree.node(root)
                    .children
                    .iter()
                    .copied()
                    .filter(|&i| !tree.node(i).is_tip()),
            )
            .collect();
        split.sort_unstable();
        if tree
            .node(root)
            .children
            .iter()
            .any(|&i| !tree.node(i).is_tip())
            && !split.is_empty()
            && split != roots
        {
            out.insert(split);
        }
        if let Some(parent) = tree.node(root).parent {
            if parent != tree.root()
                && tree
                    .node(parent)
                    .children
                    .iter()
                    .all(|&c| tree.node(c).is_tip() || roots.contains(&c))
            {
                let mut fused: Vec<_> = roots
                    .iter()
                    .copied()
                    .filter(|i| !tree.node(parent).children.contains(i))
                    .chain(std::iter::once(parent))
                    .collect();
                fused.sort_unstable();
                out.insert(fused);
            }
        }
    }
    out
}

/// Named bounded reference-neighborhood search. Uses the splits recursive
/// median/left/right starts and follows ties deterministically without cycles.
pub fn multiple(
    tree: &TreeDoc,
    time: &TimeTree,
    budget: usize,
    cancel: Option<&Path>,
) -> Result<GmycResult> {
    if budget == 0 {
        return Err(Error(
            "multi-threshold search requires a positive candidate budget".into(),
        ));
    }
    let mut cache = BTreeMap::<Vec<usize>, Fit>::new();
    let mut profile = vec![null(time)];
    let mut starts = Vec::new();
    let mut best_value = profile[0].log_likelihood;
    let mut parts = vec![
        time.events
            .iter()
            .map(|&i| time.ages[i])
            .collect::<Vec<_>>(),
    ];
    let mut exhausted = false;
    while let Some(mut ages) = parts.pop() {
        if cancelled(cancel) {
            return Err(Error("cancelled between multi-threshold searches".into()));
        }
        ages.sort_by(f64::total_cmp);
        if ages.is_empty() {
            continue;
        }
        let median = if ages.len() % 2 == 0 {
            (ages[ages.len() / 2 - 1] + ages[ages.len() / 2]) / 2.0
        } else {
            ages[ages.len() / 2]
        };
        let left: Vec<_> = ages.iter().copied().filter(|a| *a < median).collect();
        let right: Vec<_> = ages.iter().copied().filter(|a| *a >= median).collect();
        if left.is_empty() || right.is_empty() {
            continue;
        }
        let mid: Vec<_> = ages
            .iter()
            .copied()
            .filter(|a| *a >= *left.last().unwrap() && *a < right[0])
            .collect();
        let thresholds = [left[left.len() / 2], median, right[right.len() / 2]];
        let mut improvements = [0usize; 3];
        let mut local_best = best_value;
        for (part, threshold) in thresholds.into_iter().enumerate() {
            starts.push(threshold);
            let mut current = roots_at(tree, time, threshold);
            current.sort_unstable();
            let mut visited = BTreeSet::new();
            loop {
                if !visited.insert(current.clone()) {
                    break;
                }
                let proposals: Vec<_> = neighbors(tree, &current)
                    .into_iter()
                    .filter(|r| !cache.contains_key(r))
                    .collect();
                if cache.len() + proposals.len() > budget {
                    exhausted = true;
                    break;
                }
                let fits: Vec<_> = proposals
                    .par_iter()
                    .map(|roots| {
                        if cancelled(cancel) {
                            return Err(Error(
                                "cancelled between multi-threshold candidates".into(),
                            ));
                        }
                        evaluate(tree, time, roots, None)
                    })
                    .collect();
                for (roots, fit) in proposals.into_iter().zip(fits) {
                    cache.insert(roots, fit?);
                }
                let next = neighbors(tree, &current)
                    .into_iter()
                    .filter_map(|r| cache.get(&r))
                    .max_by(|a, b| {
                        a.log_likelihood
                            .total_cmp(&b.log_likelihood)
                            .then(a.roots.cmp(&b.roots))
                    });
                let Some(next) = next else {
                    break;
                };
                // The reference accepts >= and keeps the last tied proposal; visited prevents cycles.
                if current != roots_at(tree, time, threshold)
                    && cache
                        .get(&current)
                        .is_some_and(|old| old.log_likelihood > next.log_likelihood)
                {
                    break;
                }
                if next.log_likelihood > best_value {
                    improvements[part] += 1;
                    local_best = local_best.max(next.log_likelihood);
                }
                current.clone_from(&next.roots);
            }
            if exhausted {
                break;
            }
        }
        best_value = local_best;
        if exhausted {
            break;
        }
        if let Some(part) = (0..3).max_by_key(|&i| improvements[i]) {
            if improvements[part] > 0 {
                let chosen = [&left, &mid, &right][part];
                if chosen.len() < ages.len() {
                    parts.push(chosen.clone());
                }
            }
        }
    }
    profile.extend(cache.into_values());
    let mut result = result(
        "multiple",
        profile,
        false,
        if exhausted {
            "candidate budget reached; partial search is not a completed fit".into()
        } else {
            "reference split/fuse search stopped without improving recursive start".into()
        },
        starts,
    );
    if exhausted {
        result.warnings.push(
            "Search incomplete: result must be recorded as unavailable, not completed".into(),
        );
    }
    Ok(result)
}

/// Exhaustive antichain enumeration is a tiny-tree oracle, never a large-tree default.
pub fn exhaustive(
    tree: &TreeDoc,
    time: &TimeTree,
    budget: usize,
    cancel: Option<&Path>,
) -> Result<GmycResult> {
    let mut todo = vec![vec![tree.root()]];
    let mut seen = BTreeSet::new();
    let mut profile = vec![null(time)];
    while let Some(roots) = todo.pop() {
        if cancelled(cancel) {
            return Err(Error("cancelled during exhaustive GMYC".into()));
        }
        if !seen.insert(roots.clone()) {
            continue;
        }
        if seen.len() > budget {
            return Err(Error("exhaustive candidate budget reached".into()));
        }
        if roots != [tree.root()] && !roots.is_empty() {
            profile.push(evaluate(tree, time, &roots, None)?);
        }
        for &root in &roots {
            let mut next: Vec<_> = roots
                .iter()
                .copied()
                .filter(|&i| i != root)
                .chain(
                    tree.node(root)
                        .children
                        .iter()
                        .copied()
                        .filter(|&i| !tree.node(i).is_tip()),
                )
                .collect();
            next.sort_unstable();
            if !next.is_empty() {
                todo.push(next);
            }
        }
    }
    Ok(result(
        "exhaustive",
        profile,
        true,
        "all nonsingleton antichains evaluated".into(),
        vec![],
    ))
}

pub fn groups(tree: &TreeDoc, time: &TimeTree, fit: &Fit) -> Vec<BTreeSet<String>> {
    if fit.roots.is_empty() {
        return vec![time.tips.keys().cloned().collect()];
    }
    let roots: BTreeSet<_> = fit.roots.iter().copied().collect();
    let mut groups = Vec::new();
    let mut stack = vec![tree.root()];
    while let Some(i) = stack.pop() {
        if roots.contains(&i) || tree.node(i).is_tip() {
            let mut members = BTreeSet::new();
            let mut descendants = vec![i];
            while let Some(d) = descendants.pop() {
                if tree.node(d).is_tip() {
                    members.insert(tree.node(d).name.clone());
                } else {
                    descendants.extend(&tree.node(d).children);
                }
            }
            groups.push(members);
        } else {
            stack.extend(tree.node(i).children.iter().rev());
        }
    }
    groups
}

fn reduce_thresholds(tree: &TreeDoc, time: &TimeTree, roots: &[usize]) -> Vec<f64> {
    let mut pending = roots.to_vec();
    let mut thresholds = Vec::new();
    while !pending.is_empty() {
        let &oldest = pending
            .iter()
            .max_by(|&&a, &&b| time.ages[a].total_cmp(&time.ages[b]))
            .unwrap();
        let threshold = time.ages[oldest];
        thresholds.push(threshold);
        pending.retain(|&i| {
            i != oldest
                && !tree
                    .node(i)
                    .parent
                    .is_some_and(|p| time.ages[p] > threshold)
        });
    }
    thresholds
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn analytic_plugin_rates_and_waiting_likelihood() {
        let tree =
            TreeDoc::from_newick("fixture", "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);").unwrap();
        let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
        let roots = roots_at(&tree, &time, 0.2);
        let prepared = prepare(&tree, &time, &roots).unwrap();
        let l = prepared.likelihood([1.0, 1.0]).unwrap();
        // Root interval: yule=2 for 0.8; first cluster: coal=2,yule=1 for
        // 0.1; final interval: coal=4,yule=0 for 0.1. mcoal=2,mdiv=1.
        assert!((l.coal_rate.exp() - 2.0 / 0.6).abs() < 1e-12);
        assert!((l.div_rate.exp() - 1.0 / 1.7).abs() < 1e-12);
        let expected =
            (2.0 / 1.7f64).ln() + (2.0 * 2.0 / 0.6 + 1.0 / 1.7f64).ln() + (4.0 * 2.0 / 0.6f64).ln()
                - 3.0;
        assert!((l.value - expected).abs() < 1e-12);
    }
    #[test]
    fn scaling_worker_reproducibility_and_tiny_oracle() {
        let mut results = Vec::new();
        for scale in [1.0, 1000.0] {
            let tree = TreeDoc::from_newick(
                "fixture",
                &format!(
                    "((a:{},b:{}):{},(c:{},d:{}):{});",
                    0.1 * scale,
                    0.1 * scale,
                    0.9 * scale,
                    0.2 * scale,
                    0.2 * scale,
                    0.8 * scale
                ),
            )
            .unwrap();
            let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
            let one = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .unwrap()
                .install(|| single(&tree, &time, None).unwrap());
            let two = rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap()
                .install(|| single(&tree, &time, None).unwrap());
            assert_eq!(
                serde_json::to_string(&one).unwrap(),
                serde_json::to_string(&two).unwrap()
            );
            let oracle = exhaustive(&tree, &time, 100, None).unwrap();
            assert!(
                oracle.profile[oracle.best].log_likelihood + 1e-8
                    >= one.profile[one.best].log_likelihood
            );
            results.push(one);
        }
        assert_eq!(results[0].best, results[1].best);
        assert!((results[0].likelihood_ratio - results[1].likelihood_ratio).abs() < 1e-8);
    }
}
