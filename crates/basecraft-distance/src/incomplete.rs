//! Round-two driver and explicitly documented incomplete-distance research variants.
use super::{Best, Instant, json, map};
use rayon::prelude::*;
#[derive(Clone, Copy, Default)]
struct Cell {
    sum: f64,
    count: u32,
}
fn finite(x: f64) -> bool {
    x.is_finite()
}
pub(super) fn components(d: &[f64], n: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; n];
    let mut out = vec![];
    for root in 0..n {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let mut group = vec![root];
        let mut k = 0;
        while k < group.len() {
            let i = group[k];
            for j in 0..n {
                if !seen[j] && finite(d[i * n + j]) {
                    seen[j] = true;
                    group.push(j);
                }
            }
            k += 1;
        }
        group.sort_unstable();
        out.push(group);
    }
    out
}
pub(super) fn completion(d: &mut [f64], n: usize, parallel: bool) -> serde_json::Value {
    let start = Instant::now();
    let mut rounds = 0;
    let mut filled = 0;
    let mut ambiguous = 0;
    let mut witnesses = 0u64;
    loop {
        let missing: Vec<_> = (0..n)
            .flat_map(|i| (0..i).map(move |j| (i, j)))
            .filter(|&(i, j)| !finite(d[i * n + j]))
            .collect();
        if missing.is_empty() {
            break;
        }
        let out = map(missing.len(), parallel, |p| {
            let (i, j) = missing[p];
            let common: Vec<_> = (0..n)
                .filter(|&k| k != i && k != j && finite(d[i * n + k]) && finite(d[j * n + k]))
                .collect();
            let mut best = f64::INFINITY;
            let mut unique = false;
            let mut count = 0u64;
            for (a, &k) in common.iter().enumerate() {
                for &l in &common[..a] {
                    if finite(d[k * n + l]) {
                        let x = d[i * n + k] + d[j * n + l];
                        let y = d[i * n + l] + d[j * n + k];
                        let v = x.max(y) - d[k * n + l];
                        count += 1;
                        if v < best {
                            best = v;
                            unique = (x - y).abs() > 1e-10 * (1. + x.abs() + y.abs());
                        }
                    }
                }
            }
            (best, unique, count)
        });
        let mut progress = 0;
        for (&(i, j), (v, u, c)) in missing.iter().zip(out) {
            witnesses += c;
            if finite(v) {
                d[i * n + j] = v;
                d[j * n + i] = v;
                progress += 1;
                if !u {
                    ambiguous += 1;
                }
            }
        }
        if progress == 0 {
            break;
        }
        rounds += 1;
        filled += progress;
    }
    json!({"completion_s":start.elapsed().as_secs_f64(),"completion_rounds":rounds,"filled_pairs":filled,"ambiguous_minimizers":ambiguous,"quartet_witnesses":witnesses,"remaining_missing_pairs":(0..n).map(|i|(0..i).filter(|&j|!finite(d[i*n+j])).count()).sum::<usize>()})
}
fn cache_cell(d: &[f64], ids: &[usize], n: usize, i: usize, j: usize) -> Cell {
    if !finite(d[i * n + j]) {
        return Cell::default();
    }
    let mut c = Cell::default();
    for &k in ids {
        if finite(d[i * n + k]) && finite(d[j * n + k]) {
            c.sum += d[i * n + k] + d[j * n + k];
            c.count += 1;
        }
    }
    c
}
pub(super) fn direct(
    mut d: Vec<f64>,
    mut v: Vec<f64>,
    names: &[String],
    method: &str,
    parallel: bool,
    verify: bool,
) -> (Vec<String>, serde_json::Value) {
    let n = names.len();
    if v.is_empty() {
        v = vec![1.; n * n];
    }
    let mut ids: Vec<_> = (0..n).collect();
    let mut nodes: Vec<_> = names
        .iter()
        .map(|s| format!("'{}'", s.replace('\'', "''")))
        .collect();
    let mut cache = vec![Cell::default(); n * n];
    let init = Instant::now();
    let fill = |i: usize, row: &mut [Cell]| {
        for (j, cell) in row.iter_mut().enumerate().take(i) {
            *cell = cache_cell(&d, &ids, n, i, j);
        }
    };
    if parallel {
        cache
            .par_chunks_mut(n)
            .enumerate()
            .for_each(|(i, r)| fill(i, r));
    } else {
        cache
            .chunks_mut(n)
            .enumerate()
            .for_each(|(i, r)| fill(i, r));
    }
    let init_s = init.elapsed().as_secs_f64();
    let mut select_s = 0.;
    let mut update_s = 0.;
    let mut merges = 0;
    let mut check_error: f64 = 0.;
    let mut trace = vec![];
    while ids.len() > 3 {
        let start = Instant::now();
        let complete = ids
            .iter()
            .enumerate()
            .all(|(k, &i)| ids[..k].iter().all(|&j| finite(d[i * n + j])));
        let candidates = map(ids.len(), parallel, |k| {
            let i = ids[k];
            let mut row = vec![];
            for &j in &ids[..k] {
                let c = cache[i * n + j];
                if finite(d[i * n + j]) && c.count > 2 {
                    row.push(Best {
                        q: d[i * n + j] - c.sum / f64::from(c.count - 2),
                        i: j,
                        j: i,
                    });
                }
            }
            row.sort_unstable_by(|a, b| {
                a.q.total_cmp(&b.q).then(a.i.cmp(&b.i)).then(a.j.cmp(&b.j))
            });
            row.truncate(15);
            row
        });
        let mut shortlist: Vec<_> = candidates.into_iter().flatten().collect();
        shortlist
            .sort_unstable_by(|a, b| a.q.total_cmp(&b.q).then(a.i.cmp(&b.i)).then(a.j.cmp(&b.j)));
        shortlist.truncate(15);
        if shortlist.is_empty() {
            break;
        }
        let best = if complete {
            shortlist[0]
        } else {
            let scores = map(shortlist.len(), parallel, |k| {
                let b = shortlist[k];
                let (x, y) = (b.i, b.j);
                let (mut good, mut count, mut margin) = (0u64, 0u64, 0.);
                let mut diff = 0;
                for &i in &ids {
                    if i == x || i == y {
                        continue;
                    }
                    if finite(d[x * n + i]) != finite(d[y * n + i]) {
                        diff += 1;
                    }
                    for &j in &ids {
                        if j == x || j == y || j == i {
                            continue;
                        }
                        if finite(d[x * n + i]) && finite(d[y * n + j]) && finite(d[i * n + j]) {
                            let m = d[x * n + i] + d[y * n + j] - d[x * n + y] - d[i * n + j];
                            count += 1;
                            good += u64::from(m >= 0.);
                            margin += m;
                        }
                    }
                }
                (
                    if count > 0 {
                        good as f64 / count as f64
                    } else {
                        f64::NEG_INFINITY
                    },
                    count,
                    diff,
                    margin,
                    k,
                )
            });
            let rank = scores
                .into_iter()
                .max_by(|a, b| {
                    a.0.total_cmp(&b.0)
                        .then(a.1.cmp(&b.1))
                        .then(a.2.cmp(&b.2))
                        .then(a.3.total_cmp(&b.3))
                        .then(b.4.cmp(&a.4))
                })
                .unwrap();
            shortlist[rank.4]
        };
        select_s += start.elapsed().as_secs_f64();
        let start = Instant::now();
        let (x, y) = (best.i, best.j);
        let xy = d[x * n + y];
        let shared: Vec<_> = ids
            .iter()
            .copied()
            .filter(|&i| i != x && i != y && finite(d[x * n + i]) && finite(d[y * n + i]))
            .collect();
        let weights: Vec<_> = shared
            .iter()
            .map(|&i| {
                if method == "mvrstar" {
                    1. / (v[x * n + i] + v[y * n + i])
                } else {
                    1.
                }
            })
            .collect();
        let norm = 2. * weights.iter().sum::<f64>();
        let tx = 0.5 * xy
            + shared
                .iter()
                .zip(&weights)
                .map(|(&i, &w)| w / norm * (d[x * n + i] - d[y * n + i]))
                .sum::<f64>();
        let ty = xy - tx;
        if verify {
            trace.push(json!([x, y, tx, ty]));
        }
        nodes[x] = format!("({}:{tx:.12},{}:{ty:.12})", nodes[x], nodes[y]);
        let survivors: Vec<_> = ids.iter().copied().filter(|&i| i != x && i != y).collect();
        let mut nd = vec![f64::NAN; n];
        let mut nv = vec![f64::NAN; n];
        nd[x] = 0.;
        nv[x] = 0.;
        for &i in &survivors {
            let (dx, dy) = (d[x * n + i], d[y * n + i]);
            let (vx, vy) = (v[x * n + i], v[y * n + i]);
            if finite(dx) && finite(dy) {
                let lambda = if method == "mvrstar" {
                    vy / (vx + vy)
                } else {
                    0.5
                };
                nd[i] = lambda * (dx - tx) + (1. - lambda) * (dy - ty);
                nv[i] = vx * vy / (vx + vy);
            } else if finite(dx) {
                nd[i] = dx - tx;
                nv[i] = vx;
            } else if finite(dy) {
                nd[i] = dy - ty;
                nv[i] = vy;
            }
        }
        // Update every surviving pair by removing x,y contributions and adding u's contribution.
        let update = |i: usize, row: &mut [Cell]| {
            if i == x || i == y || !survivors.contains(&i) {
                return;
            }
            for &j in &survivors {
                if j >= i || !finite(d[i * n + j]) {
                    continue;
                }
                let c = &mut row[j];
                for k in [x, y] {
                    if finite(d[i * n + k]) && finite(d[j * n + k]) {
                        c.sum -= d[i * n + k] + d[j * n + k];
                        c.count -= 1;
                    }
                }
                if finite(nd[i]) && finite(nd[j]) {
                    c.sum += nd[i] + nd[j];
                    c.count += 1;
                }
            }
        };
        if parallel && ids.len() >= 64 {
            cache
                .par_chunks_mut(n)
                .enumerate()
                .for_each(|(i, r)| update(i, r));
        } else {
            cache
                .chunks_mut(n)
                .enumerate()
                .for_each(|(i, r)| update(i, r));
        }
        for &i in &survivors {
            d[x * n + i] = nd[i];
            d[i * n + x] = nd[i];
            v[x * n + i] = nv[i];
            v[i * n + x] = nv[i];
        }
        d[x * n + x] = 0.;
        v[x * n + x] = 0.;
        ids.retain(|&i| i != y);
        let fresh = map(survivors.len(), parallel && ids.len() >= 64, |k| {
            let i = survivors[k];
            (i, cache_cell(&d, &ids, n, x, i))
        });
        for (i, c) in fresh {
            cache[i.max(x) * n + i.min(x)] = c;
        }
        if verify {
            for (k, &i) in ids.iter().enumerate() {
                for &j in &ids[..k] {
                    let c = cache_cell(&d, &ids, n, i, j);
                    let old = cache[i * n + j];
                    assert_eq!(c.count, old.count);
                    check_error = check_error.max((c.sum - old.sum).abs());
                    assert!((c.sum - old.sum).abs() < 1e-9 * (1. + c.sum.abs()));
                }
            }
        }
        merges += 1;
        update_s += start.elapsed().as_secs_f64();
    }
    let complete = ids
        .iter()
        .enumerate()
        .all(|(k, &i)| ids[..k].iter().all(|&j| finite(d[i * n + j])));
    let trees = if ids.len() == 3 && complete {
        let (a, b, c) = (ids[0], ids[1], ids[2]);
        let la = (d[a * n + b] + d[a * n + c] - d[b * n + c]) / 2.;
        vec![format!(
            "({}:{la:.12},{}:{:.12},{}:{:.12});\n",
            nodes[a],
            nodes[b],
            d[a * n + b] - la,
            nodes[c],
            d[a * n + c] - la
        )]
    } else {
        ids.iter().map(|&i| format!("{};\n", nodes[i])).collect()
    };
    let unresolved = trees.len() > 1;
    (
        trees,
        json!({"init_s":init_s,"search_s":select_s,"update_s":update_s,"merges":merges,"unresolved":unresolved,"cache_max_error":check_error,"trace":trace}),
    )
}
#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    #[test]
    fn completion_quartet_and_stall() {
        let mut d = vec![
            0., 2., 6., 6., 2., 0., 6., 6., 6., 6., 0., 2., 6., 6., 2., 0.,
        ];
        d[2] = f64::NAN;
        d[8] = f64::NAN;
        completion(&mut d, 4, true);
        assert_eq!(d[2], 6.);
        let mut chain = vec![0., 1., f64::NAN, 1., 0., 1., f64::NAN, 1., 0.];
        completion(&mut chain, 3, true);
        assert!(!finite(chain[2]));
    }
    #[test]
    fn observed_unchanged_and_disconnected() {
        let mut d = vec![
            0.,
            1.,
            f64::NAN,
            f64::NAN,
            1.,
            0.,
            f64::NAN,
            f64::NAN,
            f64::NAN,
            f64::NAN,
            0.,
            2.,
            f64::NAN,
            f64::NAN,
            2.,
            0.,
        ];
        assert_eq!(components(&d, 4), vec![vec![0, 1], vec![2, 3]]);
        completion(&mut d, 4, true);
        assert_eq!(d[1], 1.);
        assert!(!finite(d[2]));
    }
    #[test]
    fn direct_additive() {
        let d = vec![
            0., 2., 6., 6., 2., 0., 6., 6., 6., 6., 0., 2., 6., 6., 2., 0.,
        ];
        let names = (0..4).map(|i| format!("s{i}")).collect::<Vec<_>>();
        for method in ["njstar", "mvrstar"] {
            let (trees, _) = direct(d.clone(), vec![1.; 16], &names, method, true, true);
            assert_eq!(trees.len(), 1);
            assert!(trees[0].contains("('s0':1.000000000000,'s1':1.000000000000):4.000000000000"));
        }
    }
}
