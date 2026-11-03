//! Size-weighted average linkage with a reciprocal nearest-neighbor chain, the method used
//! by fastcluster (Mullner 2013). O(n²) distance storage and work; ties prefer the
//! predecessor, then input order.
//!
//! A merge changes one entry in every row, a cache miss each if written at once, while the
//! chain reads only a few rows per merge. So merges go in a log, and a row replays the
//! entries it hasn't seen when it is next read. Each row's entries are then touched together,
//! and rows that merge before being read skip theirs.
use crate::optimized::{Node, emit, scan_row};
use crate::quote;

struct Merge {
    i: usize,
    j: usize,
    wi: f64,
    wj: f64,
}

fn catch_up(row: &mut [f64], merges: &[Merge]) {
    for m in merges {
        row[m.i] = m.wi * row[m.i] + m.wj * row[m.j];
        row[m.j] = f64::INFINITY;
    }
}

pub(super) fn tree(mut d: Vec<f64>, names: &[String]) -> String {
    let n = names.len();
    let mut nodes: Vec<_> = names
        .iter()
        .map(|s| Node {
            text: Some(quote(s)),
            edges: vec![],
        })
        .collect();
    let mut roots: Vec<_> = (0..n).collect();
    let mut sizes = vec![1_usize; n];
    let mut heights = vec![0.; n];
    let mut chain = Vec::new();
    // A cluster is its own worst neighbor; a merged-away cluster is everyone's.
    for i in 0..n {
        d[i * n + i] = f64::INFINITY;
    }
    let mut merges: Vec<Merge> = Vec::with_capacity(n);
    let mut seen = vec![0_usize; n];
    let zeros = vec![0.; n];
    for _ in 1..n {
        if chain.is_empty() {
            chain.push(sizes.iter().position(|&s| s > 0).unwrap());
        }
        let (i, j) = loop {
            let i = *chain.last().unwrap();
            let row = &mut d[i * n..(i + 1) * n];
            catch_up(row, &merges[seen[i]..]);
            seen[i] = merges.len();
            let nearest = scan_row(row, &zeros, 1., 0.);
            let previous = chain.iter().rev().nth(1).copied();
            // Strict comparison keeps the predecessor on equal distances.
            let best = previous
                .filter(|&p| nearest.q >= row[p])
                .unwrap_or(nearest.j);
            if previous == Some(best) {
                chain.truncate(chain.len() - 2);
                break (i.min(best), i.max(best));
            }
            chain.push(best);
        };
        let (front, back) = d.split_at_mut(j * n);
        let row_i = &mut front[i * n..(i + 1) * n];
        let row_j = &mut back[..n];
        catch_up(row_i, &merges[seen[i]..]);
        catch_up(row_j, &merges[seen[j]..]);
        let height = row_i[j] / 2.;
        nodes.push(Node {
            text: None,
            edges: vec![
                (roots[i], height - heights[i]),
                (roots[j], height - heights[j]),
            ],
        });
        roots[i] = nodes.len() - 1;
        let total = sizes[i] + sizes[j];
        let wi = sizes[i] as f64 / total as f64;
        let wj = sizes[j] as f64 / total as f64;
        // Infinity marks i, j and earlier merges, and stays infinite through this sum.
        for (a, &b) in row_i.iter_mut().zip(row_j.iter()) {
            *a = wi * *a + wj * b;
        }
        merges.push(Merge { i, j, wi, wj });
        seen[i] = merges.len();
        sizes[i] = total;
        sizes[j] = 0;
        heights[i] = height;
    }
    let root = sizes.iter().position(|&s| s > 0).unwrap();
    emit(&nodes, roots[root])
}

#[cfg(test)]
mod tests {
    use super::*;

    // The merges the previous version made, with ties settled by its rules.
    fn eager_chain(mut d: Vec<f64>, names: &[String]) -> String {
        let n = names.len();
        let mut labels: Vec<_> = names.iter().map(|s| quote(s)).collect();
        let mut sizes = vec![1_usize; n];
        let mut heights = vec![0.; n];
        let mut chain = Vec::new();
        for _ in 1..n {
            if chain.is_empty() {
                chain.push(sizes.iter().position(|&s| s > 0).unwrap());
            }
            let (i, j) = loop {
                let i = *chain.last().unwrap();
                let previous = chain.iter().rev().nth(1).copied();
                let mut best = previous.unwrap_or_else(|| {
                    sizes
                        .iter()
                        .enumerate()
                        .find(|&(j, &s)| j != i && s > 0)
                        .unwrap()
                        .0
                });
                for (j, &size) in sizes.iter().enumerate() {
                    if j != i && size > 0 && d[i * n + j] < d[i * n + best] {
                        best = j;
                    }
                }
                if previous == Some(best) {
                    chain.truncate(chain.len() - 2);
                    break (i.min(best), i.max(best));
                }
                chain.push(best);
            };
            let height = d[i * n + j] / 2.;
            labels[i] = format!(
                "({}:{},{}:{})",
                labels[i],
                crate::length_text(height - heights[i]),
                labels[j],
                crate::length_text(height - heights[j])
            );
            let total = sizes[i] + sizes[j];
            let wi = sizes[i] as f64 / total as f64;
            let wj = sizes[j] as f64 / total as f64;
            for k in 0..n {
                if sizes[k] > 0 && k != i && k != j {
                    let value = wi * d[i * n + k] + wj * d[j * n + k];
                    d[i * n + k] = value;
                    d[k * n + i] = value;
                }
            }
            sizes[i] = total;
            sizes[j] = 0;
            heights[i] = height;
        }
        format!("{};\n", labels[sizes.iter().position(|&s| s > 0).unwrap()])
    }

    #[test]
    fn lazy_updates_match_eager_merges_with_ties_and_outliers() {
        let mut state = 20_261_104_u64;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            (state >> 32) as f64 / f64::from(u32::MAX)
        };
        for n in [2, 3, 4, 5, 17, 64, 65, 66, 130, 257, 400] {
            for kind in 0..6 {
                let line: Vec<_> = (0..n).map(|_| next()).collect();
                let mut d = vec![0.; n * n];
                for i in 0..n {
                    for j in 0..i {
                        let v = match kind {
                            0 => next(),
                            1 => (next() * 4.).floor() / 4.,
                            2 => (line[i] - line[j]).abs(),
                            3 => ((line[i] - line[j]).abs() * 8.).floor() / 8.,
                            4 => 0.1,
                            _ => {
                                if i % 5 == j % 5 {
                                    0.
                                } else {
                                    0.1 + 0.01 * ((i % 5) + (j % 5)) as f64
                                }
                            }
                        };
                        d[i * n + j] = v;
                        d[j * n + i] = v;
                    }
                }
                let names: Vec<_> = (0..n).map(|i| format!("s{i}")).collect();
                assert_eq!(
                    tree(d.clone(), &names),
                    eager_chain(d, &names),
                    "n {n} kind {kind}"
                );
            }
        }
    }

    #[test]
    fn unequal_clusters_use_taxon_counts_and_ultrametric_lengths() {
        let names = ["a", "b", "c", "d"].map(String::from);
        let d = vec![
            0., 2., 4., 10., 2., 0., 4., 12., 4., 4., 0., 20., 10., 12., 20., 0.,
        ];
        assert_eq!(
            tree(d, &names),
            "((('a':1.000000000000,'b':1.000000000000):1.000000000000,'c':2.000000000000):5.000000000000,'d':7.000000000000);\n"
        );
    }

    #[test]
    fn ties_are_repeatable_and_labels_are_escaped() {
        let names = ["a'b", "b", "c"].map(String::from);
        assert_eq!(
            tree(vec![0., 2., 2., 2., 0., 2., 2., 2., 0.], &names),
            "(('a''b':1.000000000000,'b':1.000000000000):0.000000000000,'c':1.000000000000);\n"
        );
    }

    #[test]
    fn chain_matches_global_minimum_reference() {
        // Independent cubic oracle: choose the global minimum at every merge.
        let mut seed = 19_u64;
        for n in 3..24 {
            let names: Vec<_> = (0..n).map(|i| format!("taxon{i}")).collect();
            let mut d = vec![0.; n * n];
            for i in 0..n {
                for j in 0..i {
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    let v = (seed >> 12) as f64 / (1_u64 << 52) as f64;
                    d[i * n + j] = v;
                    d[j * n + i] = v;
                }
            }
            let actual = tree(d.clone(), &names);
            let mut labels: Vec<_> = names.iter().map(|s| quote(s)).collect();
            let mut clusters: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
            let mut heights = vec![0.; n];
            for _ in 1..n {
                let mut best = (f64::INFINITY, 0, 0);
                for i in 0..n {
                    for j in i + 1..n {
                        if clusters[i].is_empty() || clusters[j].is_empty() {
                            continue;
                        }
                        let sum: f64 = clusters[i]
                            .iter()
                            .flat_map(|&a| clusters[j].iter().map(move |&b| (a, b)))
                            .map(|(a, b)| d[a * n + b])
                            .sum();
                        let v = sum / (clusters[i].len() * clusters[j].len()) as f64;
                        if v < best.0 {
                            best = (v, i, j);
                        }
                    }
                }
                let (v, i, j) = best;
                let h = v / 2.;
                labels[i] = format!(
                    "({}:{},{}:{})",
                    labels[i],
                    crate::length_text(h - heights[i]),
                    labels[j],
                    crate::length_text(h - heights[j])
                );
                heights[i] = h;
                let members = std::mem::take(&mut clusters[j]);
                clusters[i].extend(members);
            }
            assert_eq!(actual, format!("{};\n", labels[0]), "n={n}");
        }
    }
}
