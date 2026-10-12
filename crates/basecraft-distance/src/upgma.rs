//! Size-weighted average linkage with a reciprocal nearest-neighbor chain.
//! O(n²) distance storage and work; ties prefer the predecessor, then input order.
use crate::quote;

pub(super) fn tree(mut d: Vec<f64>, names: &[String]) -> String {
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
                // Strict comparison keeps the predecessor on equal distances.
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
            "({}:{:.12},{}:{:.12})",
            labels[i],
            height - heights[i],
            labels[j],
            height - heights[j]
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

#[cfg(test)]
mod tests {
    use super::*;

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
                    "({}:{:.12},{}:{:.12})",
                    labels[i],
                    h - heights[i],
                    labels[j],
                    h - heights[j]
                );
                heights[i] = h;
                let members = std::mem::take(&mut clusters[j]);
                clusters[i].extend(members);
            }
            assert_eq!(actual, format!("{};\n", labels[0]), "n={n}");
        }
    }
}
