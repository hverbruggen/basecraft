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
    pub lr_p_value: f64,
    pub support_candidates: Vec<usize>,
    pub support_convention: String,
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
    if prepared.bases.iter().filter(|&&base| base > 0.0).count() == 1 {
        diagnostics.push("coalescent exponent and rate are not separately identified: all positive lineage-count bases are equal".into());
    }
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

fn validate_events(time: &TimeTree) -> Result<()> {
    if time.ages.iter().any(|age| !age.is_finite())
        || time.events.iter().any(|&i| time.ages[i] <= 0.0)
    {
        return Err(Error(
            "GMYC requires positive event ages; zero-time events are not jittered".into(),
        ));
    }
    Ok(())
}

/// Split/fuse and exhaustive antichains can cut inside a zero-duration
/// ancestral interval. A lineage-count maximum confined to that interval can
/// give event density without exposure, making the likelihood unbounded.
/// Single-threshold cuts keep the entire tied event together instead.
fn validate_multithreshold_events(tree: &TreeDoc, time: &TimeTree) -> Result<()> {
    if time.events.iter().any(|&i| {
        tree.node(i)
            .parent
            .is_some_and(|p| time.ages[p].total_cmp(&time.ages[i]).is_eq())
    }) {
        return Err(Error(
            "multi-threshold GMYC does not support nested tied branching events: partitions through zero-duration ancestral intervals can have unbounded likelihood; use single-threshold GMYC".into(),
        ));
    }
    Ok(())
}

pub fn single(tree: &TreeDoc, time: &TimeTree, cancel: Option<&Path>) -> Result<GmycResult> {
    single_with_progress(tree, time, cancel, &|_, _| {})
}
pub fn single_with_progress(
    tree: &TreeDoc,
    time: &TimeTree,
    cancel: Option<&Path>,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<GmycResult> {
    validate_events(time)?;
    let mut profile = vec![null(time)];
    let mut seen = BTreeSet::new();
    let candidates: Vec<_> = time
        .events
        .iter()
        .skip(1)
        .filter_map(|&i| {
            let roots = roots_at(tree, time, time.ages[i]);
            // A root-age tie has no mixed partition: the whole tree is already
            // represented by the null fit, not an empty set of cluster roots.
            (!roots.is_empty() && seen.insert(roots.clone())).then_some((roots, time.ages[i]))
        })
        .collect();
    let completed = std::sync::atomic::AtomicUsize::new(0);
    progress(0, candidates.len());
    let fits: Vec<_> = candidates
        .par_iter()
        .map(|(roots, threshold)| {
            if cancelled(cancel) {
                return Err(Error("cancelled between GMYC candidates".into()));
            }
            let fit = evaluate(tree, time, roots, Some(*threshold));
            let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            progress(done, candidates.len());
            fit
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
    let near_tied = profile.iter().enumerate().any(|(i, fit)| {
        i != best && (fit.log_likelihood - profile[best].log_likelihood).abs() < 1e-6
    });
    let mut warnings = vec!["Convergence does not establish statistical support or biological correctness".into(),"Recent-age clamp in splits is not applied; compatibility comparisons must record it explicitly".into(),"Native rates are evaluated at the returned exponents; splits reports its last objective-call rate cache, which can differ from its returned optimum".into()];
    if near_tied {
        warnings.push("Alternative partitions differ by less than 1e-6 log-likelihood units; numerical optimizer tolerance can change the selected partition. Inspect the full support profile".into());
    }
    let likelihood_ratio = 2.0 * (profile[best].log_likelihood - profile[0].log_likelihood);
    let evaluated_candidates = profile.len();
    // splits 1.0-20 uses the revised df=2 convention for both methods.
    let degrees = 2;
    let support_candidates = (0..profile.len())
        .filter(|&i| profile[i].log_likelihood > profile[best].log_likelihood - 2.0)
        .collect();
    GmycResult { method:method.into(),profile,best,likelihood_ratio,lr_degrees_of_freedom:degrees,lr_p_value:chi_square_tail(likelihood_ratio,degrees),support_candidates,support_convention:"splits 1.0-20 summary convention: log likelihood strictly greater than maximum minus 2; asymptotic chi-square LR test".into(),reference:"splits 1.0-20 plug-in pooled-rate waiting-time convention; native unmodified node ages".into(),evaluated_candidates,global_optimum,stopping_condition,starts,warnings }
}

/// The newer reference changes its LR summary only; fitted likelihoods are unchanged.
pub fn use_newer_summary(result: &mut GmycResult) {
    result.lr_degrees_of_freedom = if result.method == "single" {
        3
    } else {
        2 + result.profile[result.best].thresholds.len()
    };
    result.lr_p_value = chi_square_tail(result.likelihood_ratio, result.lr_degrees_of_freedom);
    result.support_convention = "splits 1.1 dc187fd summary convention: log likelihood strictly greater than maximum minus 2; chi-square df=3 single or 2+threshold count multiple".into();
    result.reference = "splits 1.1 dc187fd71ec583b514e4068189316fb50568b1f4; plug-in pooled-rate waiting-time convention; native unmodified node ages".into();
}

/// Upper-tail chi-square probability. Integer and half-integer gamma shapes
/// admit a stable recurrence; the initial erfc is evaluated by convergent
/// incomplete-gamma series/continued fraction, rather than a normal shortcut.
fn chi_square_tail(statistic: f64, degrees: usize) -> f64 {
    if statistic <= 0.0 || degrees == 0 {
        return 1.0;
    }
    let shape = degrees as f64 / 2.0;
    let x = statistic / 2.0;
    let mut log_gamma = if degrees.is_multiple_of(2) {
        0.0
    } else {
        std::f64::consts::PI.ln() / 2.0
    };
    let mut term = if degrees.is_multiple_of(2) { 1.0 } else { 0.5 };
    while term < shape {
        log_gamma += term.ln();
        term += 1.0;
    }
    let scale = (-x + shape * x.ln() - log_gamma).exp();
    if x < shape + 1.0 {
        let mut sum = 1.0 / shape;
        let mut add = sum;
        let mut denominator = shape;
        for _ in 0..10_000 {
            denominator += 1.0;
            add *= x / denominator;
            sum += add;
            if add.abs() < sum.abs() * 1e-15 {
                break;
            }
        }
        (1.0 - sum * scale).clamp(0.0, 1.0)
    } else {
        let mut b = x + 1.0 - shape;
        let mut c = 1e300;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..10_000 {
            let index = f64::from(i);
            let a = -index * (index - shape);
            b += 2.0;
            d = a * d + b;
            if d.abs() < 1e-300 {
                d = 1e-300;
            }
            c = b + a / c;
            if c.abs() < 1e-300 {
                c = 1e-300;
            }
            d = 1.0 / d;
            let delta = d * c;
            h *= delta;
            if (delta - 1.0).abs() < 1e-15 {
                break;
            }
        }
        (scale * h).clamp(0.0, 1.0)
    }
}

/// Preserve reference proposal order: fusions first, then divisions. Cache keys
/// are canonical sets, but traversal retains the order in each proposal.
fn neighbors(tree: &TreeDoc, roots: &[usize]) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    if roots.len() <= 1 {
        return out;
    }
    for &root in roots {
        if let Some(parent) = tree.node(root).parent
            && parent != tree.root()
                && tree
                    .node(parent)
                    .children
                    .iter()
                    .all(|&c| tree.node(c).is_tip() || roots.contains(&c))
            {
                let fused: Vec<_> = roots
                    .iter()
                    .copied()
                    .filter(|i| !tree.node(parent).children.contains(i))
                    .chain(std::iter::once(parent))
                    .collect();
                if !out.contains(&fused) {
                    out.push(fused);
                }
            }
    }
    for &root in roots {
        if tree
            .node(root)
            .children
            .iter()
            .any(|&i| !tree.node(i).is_tip())
        {
            out.push(
                roots
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
                    .collect(),
            );
        }
    }
    out
}
fn canonical(roots: &[usize]) -> Vec<usize> {
    let mut key = roots.to_vec();
    key.sort_unstable();
    key
}
fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

/// Reference recursive mean/left/right starts and ordered split/fuse traversal.
/// Duplicate numerical evaluations are cached; an explicit budget bounds work.
pub fn multiple(
    tree: &TreeDoc,
    time: &TimeTree,
    budget: usize,
    cancel: Option<&Path>,
) -> Result<GmycResult> {
    multiple_with_progress(tree, time, budget, cancel, &|_, _| {})
}
pub fn multiple_with_progress(
    tree: &TreeDoc,
    time: &TimeTree,
    budget: usize,
    cancel: Option<&Path>,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<GmycResult> {
    validate_events(time)?;
    validate_multithreshold_events(tree, time)?;
    if budget == 0 {
        return Err(Error(
            "multi-threshold search requires a positive candidate budget".into(),
        ));
    }
    let mut cache = BTreeMap::<Vec<usize>, Fit>::new();
    let mut profile = vec![null(time)];
    let mut starts = Vec::new();
    let mut baseline = profile[0].log_likelihood;
    let mut ages = time
        .events
        .iter()
        .map(|&i| time.ages[i])
        .collect::<Vec<_>>();
    let mut exhausted = false;
    let mut cycles = false;
    progress(0, budget);
    loop {
        if cancelled(cancel) {
            return Err(Error("cancelled between multi-threshold searches".into()));
        }
        if ages.is_empty() {
            break;
        }
        let middle = mean(&ages);
        // R works in negative ages; its left subset is the older positive ages.
        let older: Vec<_> = ages.iter().copied().filter(|a| *a > middle).collect();
        let younger: Vec<_> = ages.iter().copied().filter(|a| *a <= middle).collect();
        if older.is_empty() || younger.is_empty() {
            break;
        }
        let old_mean = mean(&older);
        let young_mean = mean(&younger);
        let mid: Vec<_> = ages
            .iter()
            .copied()
            .filter(|a| *a <= old_mean && *a > young_mean)
            .collect();
        let thresholds = [old_mean, middle, young_mean];
        let mut improvements = [0usize; 3];
        for (part, threshold) in thresholds.into_iter().enumerate() {
            starts.push(threshold);
            let mut current = roots_at(tree, time, threshold);
            let mut visited = BTreeSet::new();
            let mut maximum: Option<f64> = None;
            loop {
                if !visited.insert(canonical(&current)) {
                    cycles = true;
                    break;
                }
                let proposals = neighbors(tree, &current);
                let missing: BTreeSet<_> = proposals
                    .iter()
                    .map(|r| canonical(r))
                    .filter(|key| !cache.contains_key(key))
                    .collect();
                if cache.len() + missing.len() > budget {
                    exhausted = true;
                    break;
                }
                let missing: Vec<_> = missing.into_iter().collect();
                let fits: Vec<_> = missing
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
                for (key, fit) in missing.into_iter().zip(fits) {
                    cache.insert(key, fit?);
                }
                progress(cache.len(), budget);
                let mut next = None;
                // Every running improvement is counted by the R recursion,
                // including an intermediate proposal superseded in this sweep.
                for proposal in proposals {
                    let fit = &cache[&canonical(&proposal)];
                    if maximum.is_none_or(|old| fit.log_likelihood >= old) {
                        maximum = Some(fit.log_likelihood);
                        if fit.log_likelihood > baseline {
                            profile.push(fit.clone());
                            improvements[part] += 1;
                        }
                        next = Some(proposal);
                    }
                }
                let Some(next) = next else {
                    break;
                };
                current = next;
            }
            if exhausted {
                break;
            }
        }
        if exhausted {
            break;
        }
        // which.max in R takes the first tied maximum.
        let part = (0..3)
            .max_by_key(|&i| (improvements[i], std::cmp::Reverse(i)))
            .unwrap();
        if improvements[part] == 0 {
            break;
        }
        baseline = profile
            .iter()
            .map(|fit| fit.log_likelihood)
            .fold(baseline, f64::max);
        let chosen = [&older, &mid, &younger][part];
        if chosen.len() >= ages.len() {
            break;
        }
        ages.clone_from(chosen);
    }
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
    result.evaluated_candidates = cache.len() + 1;
    if exhausted {
        result.warnings.push(
            "Search incomplete: result must be recorded as unavailable, not completed".into(),
        );
    }
    if cycles {
        result.warnings.push("Exact tied split/fuse cycle stopped after revisiting a partition; reference traversal can cycle".into());
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
    validate_events(time)?;
    validate_multithreshold_events(tree, time)?;
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

/// Each tip of a collapsed tree to the original tips it stands for.
pub type Merged = BTreeMap<String, BTreeSet<String>>;

/// Tips at zero distance from each other meet at an event of age zero,
/// which GMYC cannot fit: with missing data or ambiguities, sequences that
/// differ only there are separate haplotypes but have no distance between
/// them. Each largest such clade becomes one tip, named after its first tip
/// by name. Returns that tree and each new tip's original tips, or `None`
/// when every event is older than zero.
pub fn collapse_zero_age(tree: &TreeDoc, time: &TimeTree) -> Result<Option<(TreeDoc, Merged)>> {
    let zero = |i: usize| !tree.node(i).is_tip() && time.ages[i] <= 0.0;
    if !time.events.iter().any(|&i| time.ages[i] <= 0.0) {
        return Ok(None);
    }
    if zero(tree.root()) {
        return Err(Error(
            "GMYC unavailable: every retained sequence is at zero distance from the others".into(),
        ));
    }
    let mut text = vec![String::new(); tree.nodes().len()];
    let mut first = vec![String::new(); tree.nodes().len()];
    let mut merged = BTreeMap::new();
    for &i in tree.preorder().iter().rev() {
        let node = tree.node(i);
        let length = node.length.map(|l| format!(":{l}")).unwrap_or_default();
        if node.is_tip() {
            first[i].clone_from(&node.name);
            text[i] = basecraft_io::newick::quote_name(&node.name) + &length;
            continue;
        }
        first[i] = node
            .children
            .iter()
            .map(|&c| first[c].clone())
            .min()
            .unwrap_or_default();
        if zero(i) {
            if node.parent.is_some_and(|p| !zero(p)) {
                let mut members = BTreeSet::new();
                let mut stack = vec![i];
                while let Some(d) = stack.pop() {
                    if tree.node(d).is_tip() {
                        members.insert(tree.node(d).name.clone());
                    } else {
                        stack.extend(&tree.node(d).children);
                    }
                }
                merged.insert(first[i].clone(), members);
            }
            text[i] = basecraft_io::newick::quote_name(&first[i]) + &length;
        } else {
            let children: Vec<&str> = node.children.iter().map(|&c| text[c].as_str()).collect();
            text[i] = format!("({}){length}", children.join(","));
        }
    }
    let collapsed = TreeDoc::from_newick(tree.name.clone(), &format!("{};", text[tree.root()]))
        .map_err(|e| Error(e.to_string()))?;
    Ok(Some((collapsed, merged)))
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
    fn root_age_ties_keep_null_and_admissible_mixed_partitions() {
        for newick in ["((a:1,b:1):0,c:1);", "(((a:0.2,b:0.2):0.8,c:1):0,d:1);"] {
            let tree = TreeDoc::from_newick("root tie", newick).unwrap();
            let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
            let fit = single(&tree, &time, None).unwrap();
            assert_eq!(fit.profile.len(), if time.tips.len() == 3 { 1 } else { 2 });
            assert!(fit.profile.iter().all(|fit| fit.log_likelihood.is_finite()));
        }
    }

    #[test]
    fn nested_tie_has_zero_exposure_but_retains_event_density() {
        let tree = TreeDoc::from_newick(
            "nested tie",
            "(((a:0.2,b:0.2):0,c:0.2):0.8,(d:0.1,e:0.1):0.9);",
        )
        .unwrap();
        let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
        let prepared = prepare(&tree, &time, &roots_at(&tree, &time, 0.2)).unwrap();
        let likelihood = prepared.likelihood([1.0, 1.0]).unwrap();
        // After the root: (coal,div)=(0,2),(2,1),(6,1),(8,0),
        // for durations 0.8,0,0.1,0.1. The tied event still has a density.
        let coal = 3.0 / 1.4;
        let div = 1.0 / 1.7;
        let expected = (2.0_f64 * div).ln()
            + (2.0_f64 * coal + div).ln()
            + (6.0_f64 * coal + div).ln()
            + (8.0_f64 * coal).ln()
            - 4.0;
        assert!((likelihood.coal_rate.exp() - coal).abs() < 1e-12);
        assert!((likelihood.div_rate.exp() - div).abs() < 1e-12);
        assert!((likelihood.value - expected).abs() < 1e-12);
    }

    #[test]
    fn zero_distance_clades_become_one_tip_each() {
        let tree = TreeDoc::from_newick(
            "t",
            "(((b:0,a:0):0,'c d':0):2,((e:1,f:1):0.5,(g:0,h:0):1.5):0.5);",
        )
        .unwrap();
        let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
        assert!(validate_events(&time).is_err());
        let (collapsed, merged) = collapse_zero_age(&tree, &time).unwrap().unwrap();
        let tips: Vec<_> = collapsed
            .tips_in_order()
            .iter()
            .map(|&i| collapsed.node(i).name.clone())
            .collect();
        assert_eq!(tips, ["a", "e", "f", "g"]);
        assert_eq!(merged["a"], ["a", "b", "c d"].map(String::from).into());
        assert_eq!(merged["g"], ["g", "h"].map(String::from).into());
        let time = crate::time_tree::validate(&collapsed, true, 1e-8, true).unwrap();
        validate_events(&time).unwrap();
        assert!((time.root_height - 2.0).abs() < 1e-12);
        let tidy = TreeDoc::from_newick("t", "((a:1,b:1):1,(c:2,d:2):0);").unwrap();
        let time = crate::time_tree::validate(&tidy, true, 1e-8, true).unwrap();
        assert!(collapse_zero_age(&tidy, &time).unwrap().is_none());
    }
    #[test]
    fn zero_age_collapse_preserves_surviving_paths_and_positive_events() {
        let tree = TreeDoc::from_newick(
            "paths",
            "(((b:0,'a quoted':0):0,c:0):2,((e:1,f:1):0.5,(g:0,h:0):1.5):0.5);",
        )
        .unwrap();
        let original = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
        let (collapsed, _) = collapse_zero_age(&tree, &original).unwrap().unwrap();
        let retained = crate::time_tree::validate(&collapsed, true, 1e-8, true).unwrap();
        assert_eq!(original.root_height, retained.root_height);
        let positive: Vec<_> = original
            .events
            .iter()
            .map(|&i| original.ages[i])
            .filter(|&age| age > 0.0)
            .collect();
        assert_eq!(
            positive,
            retained
                .events
                .iter()
                .map(|&i| retained.ages[i])
                .collect::<Vec<_>>()
        );
        // In an ultrametric tree each pairwise path is twice its MRCA age.
        // Checking every retained pair also checks the surviving hierarchy.
        let mrca_age = |tree: &TreeDoc, time: &TimeTree, a: &str, b: &str| {
            let mut ancestors = BTreeSet::new();
            let mut node = Some(time.tips[a]);
            while let Some(i) = node {
                ancestors.insert(i);
                node = tree.node(i).parent;
            }
            let mut i = time.tips[b];
            while !ancestors.contains(&i) {
                i = tree.node(i).parent.unwrap();
            }
            time.ages[i]
        };
        for a in retained.tips.keys() {
            for b in retained.tips.keys() {
                assert_eq!(
                    mrca_age(&tree, &original, a, b),
                    mrca_age(&collapsed, &retained, a, b)
                );
            }
        }
        let short =
            TreeDoc::from_newick("short", "((a:0.000000001,b:0.000000001):0.999999999,c:1);")
                .unwrap();
        let time = crate::time_tree::validate(&short, true, 1e-8, true).unwrap();
        assert!(collapse_zero_age(&short, &time).unwrap().is_none());
        let all_zero = TreeDoc::from_newick("zero", "((a:0,b:0):0,c:0);").unwrap();
        assert!(
            crate::time_tree::validate(&all_zero, true, 1e-8, true)
                .unwrap_err()
                .0
                .contains("positive finite root height")
        );
    }

    #[test]
    fn multiple_positive_intervals_are_reproducible_and_bounded_by_oracle() {
        let tree = TreeDoc::from_newick(
            "nested ties",
            "(((a:0.15,b:0.15):0.05,c:0.2):0.8,(d:0.1,e:0.1):0.9);",
        )
        .unwrap();
        let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
        let fit = |workers| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap()
                .install(|| multiple(&tree, &time, 100, None).unwrap())
        };
        let one = fit(1);
        assert_eq!(
            serde_json::to_value(&one).unwrap(),
            serde_json::to_value(fit(2)).unwrap()
        );
        assert!(!one.global_optimum);
        assert!(!one.stopping_condition.contains("budget reached"));
        assert!(one.profile.iter().all(|fit| fit.log_likelihood.is_finite()));
        let oracle = exhaustive(&tree, &time, 100, None).unwrap();
        assert!(
            oracle.profile[oracle.best].log_likelihood + 1e-8
                >= one.profile[one.best].log_likelihood
        );
        assert!(
            multiple(&tree, &time, 0, None)
                .unwrap_err()
                .0
                .contains("positive candidate budget")
        );
        let partial = multiple(&tree, &time, 1, None).unwrap();
        assert!(partial.stopping_condition.contains("partial search"));
        assert!(partial.warnings.iter().any(|w| w.contains("unavailable")));
    }

    #[test]
    fn splitting_a_nested_tie_has_unbounded_event_density() {
        let tree = TreeDoc::from_newick(
            "unbounded split",
            "(((a:0.2,b:0.2):0,c:0.2):0.8,(d:0.1,e:0.1):0.9);",
        )
        .unwrap();
        let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
        let roots = [
            tree.node(time.tips["a"]).parent.unwrap(),
            tree.node(time.tips["d"]).parent.unwrap(),
        ];
        let prepared = prepare(&tree, &time, &roots).unwrap();
        assert_eq!(prepared.div, [2.0, 3.0, 2.0, 1.0]);
        assert_eq!(prepared.dt[1], 0.0);
        // Only the empty interval has diversification base 3. All positive
        // exposure uses bases <= 2: density grows as p_div * ln(3/2).
        let low = prepared.likelihood([1.0, 10.0]).unwrap().value;
        let high = prepared.likelihood([1.0, 100.0]).unwrap().value;
        assert!((high - low - 90.0 * (1.5_f64).ln()).abs() < 0.1);
    }

    #[test]
    fn multithreshold_rejects_nested_ties_but_preserves_separate_equal_ages() {
        for newick in [
            "(((a:0.2,b:0.2):0,c:0.2):0.8,(d:0.1,e:0.1):0.9);",
            "((a:0.2,(b:0.2,c:0.2):0):0.8,(d:0.1,e:0.1):0.9);",
            "((a:1,b:1):0,c:1);",
            "(a:1,(b:1,c:1):0);",
            "(((a:0.2,b:0.2):0.8,c:1):0,d:1);",
        ] {
            let tree = TreeDoc::from_newick("nested", newick).unwrap();
            let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
            assert!(single(&tree, &time, None).is_ok());
            for error in [
                multiple(&tree, &time, 100, None).unwrap_err(),
                exhaustive(&tree, &time, 100, None).unwrap_err(),
            ] {
                assert!(error.0.contains("nested tied branching events"));
                assert!(error.0.contains("unbounded likelihood"));
            }
        }
        for newick in [
            "((a:0.2,b:0.2):0.8,(c:0.2,d:0.2):0.8);",
            "(((a:0.199999999,b:0.199999999):0.000000001,c:0.2):0.8,(d:0.1,e:0.1):0.9);",
        ] {
            let tree = TreeDoc::from_newick("supported", newick).unwrap();
            let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
            assert!(multiple(&tree, &time, 100, None).is_ok());
            assert!(exhaustive(&tree, &time, 100, None).is_ok());
        }
    }

    #[test]
    fn reference_summary_chi_square_convention_has_known_tails() {
        assert!((chi_square_tail(2.0, 2) - (-1.0f64).exp()).abs() < 1e-14);
        assert!((chi_square_tail(3.841_458_820_694_124, 1) - 0.05).abs() < 1e-12);
        assert!((chi_square_tail(7.814_727_903_251_179, 3) - 0.05).abs() < 1e-12);
        assert!((chi_square_tail(9.487_729_036_781_154, 4) - 0.05).abs() < 1e-12);
    }

    #[test]
    fn reference_versions_report_distinct_summary_conventions() {
        let tree =
            TreeDoc::from_newick("fixture", "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);").unwrap();
        let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
        let mut fit = single(&tree, &time, None).unwrap();
        assert_eq!(fit.lr_degrees_of_freedom, 2);
        let old = fit.lr_p_value;
        let best = fit.best;
        use_newer_summary(&mut fit);
        assert_eq!(fit.best, best);
        assert_eq!(fit.lr_degrees_of_freedom, 3);
        assert!(fit.lr_p_value > old);
        assert!(
            fit.profile[fit.best]
                .diagnostics
                .iter()
                .any(|text| text.contains("not separately identified"))
        );
    }

    #[test]
    fn numerically_tied_partitions_are_reported_without_forced_reassignment() {
        let tree = TreeDoc::from_newick(
            "ladder",
            include_str!("../../../testdata/fixtures/delimitation/gmyc-ladder.nwk"),
        )
        .unwrap();
        let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
        let fit = single(&tree, &time, None).unwrap();
        assert!(
            fit.warnings
                .iter()
                .any(|warning| warning.contains("Alternative partitions"))
        );
        assert!(fit.support_candidates.len() > 1);
        assert!(fit.likelihood_ratio < 1e-6);
    }

    #[test]
    fn upgma_tie_fits_independently_of_its_binary_resolution() {
        // Deduplicated UPGMA tree from the compact example: 03, 04 and 05 are
        // equidistant, so UPGMA emits a zero-length internal branch.
        let fit = |newick: &str| {
            let tree = TreeDoc::from_newick("tie", newick).unwrap();
            let time = crate::time_tree::validate(&tree, true, 1e-8, true).unwrap();
            let fit = single(&tree, &time, None).unwrap();
            let best = &fit.profile[fit.best];
            let groups: BTreeSet<_> = groups(&tree, &time, best).into_iter().collect();
            (best.log_likelihood, groups)
        };
        let (left, left_groups) = fit(
            "(('s01':0.051590136558,('s06':0.002840939663,'s07':0.002840939663):0.048749196896):0.002580294583,(('s03':0.005618213963,'s04':0.005618213963):0.000000000000,'s05':0.005618213963):0.048552217178);",
        );
        let (right, right_groups) = fit(
            "(('s01':0.051590136558,('s06':0.002840939663,'s07':0.002840939663):0.048749196896):0.002580294583,('s03':0.005618213963,('s04':0.005618213963,'s05':0.005618213963):0.000000000000):0.048552217178);",
        );
        assert!(left.is_finite());
        assert!((left - right).abs() < 1e-9);
        assert_eq!(left_groups, right_groups);
    }

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
