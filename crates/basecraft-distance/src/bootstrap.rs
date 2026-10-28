//! Exact ordinary site bootstrap. No pair-pattern cache or candidate-tree approximation.
use crate::{
    Alignment, DistanceMatrix, Method, Packed, Result, TreeResult, estimate, optimized, pool,
};
use basecraft_io::newick::{ParsedTree, parse_newick};
use rayon::prelude::*;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::{BufWriter, Write as _},
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, Debug)]
pub struct BootstrapOptions {
    pub replicates: usize,
    pub seed: u64,
    pub method: Method,
    pub distance_threads: usize,
    pub tree_threads: usize,
    /// Atomic progress snapshots; callers can poll this through the public plugin parameters.
    pub progress: Option<PathBuf>,
    /// Creating this file requests cancellation between replicates.
    pub cancel_file: Option<PathBuf>,
    /// Stream successful replicates to a new NEXUS file; never replace an existing file.
    pub replicate_trees: Option<PathBuf>,
}
#[derive(Debug, Serialize)]
pub struct ReplicateCollection {
    pub path: PathBuf,
    pub format: &'static str,
    pub status: &'static str,
    pub trees: usize,
}
#[derive(Debug, Serialize)]
pub struct BootstrapReport {
    pub seed: u64,
    pub requested: usize,
    pub completed: usize,
    pub attempted: usize,
    pub method: Method,
    pub support_kind: &'static str,
    pub support_scale: &'static str,
    pub support_withheld: bool,
    pub cancelled: bool,
    pub failures: Vec<String>,
    pub failed_replicates: usize,
    pub distance_s: f64,
    pub tree_s: f64,
    pub total_s: f64,
    pub algorithm: &'static str,
    pub rng: &'static str,
    /// Set only after atomic publication. Progress never advertises scratch files.
    pub replicate_trees: Option<ReplicateCollection>,
}
#[derive(Debug)]
pub struct BootstrapResult {
    pub tree: Option<TreeResult>,
    pub report: BootstrapReport,
}

/// A collection was published by this invocation, but the final progress write failed.
/// Callers must preserve this evidence even though the operation returned an error.
#[derive(Debug)]
pub struct BootstrapPublicationError {
    pub report: BootstrapReport,
    cause: Box<dyn std::error::Error + Send + Sync>,
}
impl std::fmt::Display for BootstrapPublicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "final bootstrap progress write failed: {}", self.cause)
    }
}
impl std::error::Error for BootstrapPublicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

/// A partial collection has a distinct name even when all attempts finished.
pub fn partial_collection_path(path: &Path) -> PathBuf {
    let mut name = path.file_stem().unwrap_or_default().to_os_string();
    name.push(".partial.nex");
    path.with_file_name(name)
}

struct CollectionWriter {
    file: BufWriter<tempfile::NamedTempFile>,
    destination: PathBuf,
}
impl CollectionWriter {
    fn new(path: &Path, options: &BootstrapOptions) -> Result<Self> {
        for candidate in [path.to_path_buf(), partial_collection_path(path)] {
            if candidate.try_exists()? || candidate.symlink_metadata().is_ok() {
                return Err(format!(
                    "bootstrap collection already exists: {}",
                    candidate.display()
                )
                .into());
            }
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = BufWriter::new(tempfile::NamedTempFile::new_in(parent)?);
        writeln!(
            file,
            "#NEXUS\n[Basecraft ordinary site bootstrap; seed={}; requested={}]\nBEGIN TREES;",
            options.seed, options.replicates
        )?;
        Ok(Self {
            file,
            destination: path.to_path_buf(),
        })
    }

    fn tree(&mut self, replicate: usize, newick: &str, rooted: bool) -> Result<()> {
        writeln!(
            self.file,
            "TREE replicate_{} = [&{}] {}",
            replicate + 1,
            if rooted { "R" } else { "U" },
            newick.trim()
        )?;
        Ok(())
    }

    fn finish(mut self, report: &BootstrapReport) -> Result<ReplicateCollection> {
        let status = if report.cancelled {
            "cancelled"
        } else if report.attempted == 0 {
            "original_failed"
        } else if report.support_withheld {
            "partial"
        } else {
            "complete"
        };
        writeln!(
            self.file,
            "END;\n[status={status}; requested={}; attempted={}; retained={}; failed={}; support_withheld={}]",
            report.requested,
            report.attempted,
            report.completed,
            report.failed_replicates,
            report.support_withheld
        )?;
        self.file.flush()?;
        let temporary = self.file.into_inner()?;
        temporary.as_file().sync_all()?;
        let path = if report.support_withheld {
            partial_collection_path(&self.destination)
        } else {
            self.destination
        };
        temporary.persist_noclobber(&path)?;
        Ok(ReplicateCollection {
            path,
            format: "nexus",
            status,
            trees: report.completed,
        })
    }
}

fn complete_tree(tree: TreeResult) -> Result<TreeResult> {
    if tree.trees.len() != 1 || tree.components != 1 || tree.unresolved {
        return Err("bootstrap requires one complete tree; disconnected or unresolved forests are unsupported".into());
    }
    Ok(tree)
}

// SplitMix64 and unbiased rejection sampling are specified here rather than depending on
// a dependency's evolving RNG implementation. Replicate r has its own derived seed.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
fn multiplicities(width: usize, seed: u64, replicate: usize) -> Vec<u64> {
    let mut state = seed.wrapping_add((replicate as u64).wrapping_mul(0xd134_2543_de82_ef95));
    let mut weights = vec![0; width];
    let bound = width as u64;
    let threshold = bound.wrapping_neg() % bound;
    for _ in 0..width {
        let value = loop {
            let x = next(&mut state);
            if x >= threshold {
                break x % bound;
            }
        };
        weights[usize::try_from(value).unwrap()] += 1;
    }
    weights
}
fn planes(weights: &[u64]) -> Vec<Vec<u64>> {
    let bits = 64 - weights.iter().max().unwrap_or(&0).leading_zeros();
    (0..bits)
        .map(|bit| {
            let mut plane = vec![0; weights.len().div_ceil(64)];
            for (i, &weight) in weights.iter().enumerate() {
                plane[i / 64] |= ((weight >> bit) & 1) << (i % 64);
            }
            plane
        })
        .collect()
}
fn weighted_counts(a: &Packed, b: &Packed, weights: &[Vec<u64>]) -> (u64, u64, u64) {
    let (mut valid, mut ts, mut tv) = (0, 0, 0);
    for (bit, plane) in weights.iter().enumerate() {
        for (k, &weight) in plane.iter().enumerate() {
            let mask = a.valid[k] & b.valid[k] & weight;
            let h = (a.hi[k] ^ b.hi[k]) & mask;
            let l = (a.lo[k] ^ b.lo[k]) & mask;
            valid += u64::from(mask.count_ones()) << bit;
            tv += u64::from(h.count_ones()) << bit;
            ts += u64::from((l & !h).count_ones()) << bit;
        }
    }
    (valid, ts, tv)
}
fn weighted_matrix(
    original: &DistanceMatrix,
    packed: &[Packed],
    weights: &[u64],
    workers: &rayon::ThreadPool,
) -> DistanceMatrix {
    let planes = planes(weights);
    counted_matrix(original, packed, workers, |a, b| {
        weighted_counts(a, b, &planes)
    })
}
fn resampled_matrix(
    original: &DistanceMatrix,
    alignment: &Alignment,
    weights: &[u64],
    workers: &rayon::ThreadPool,
) -> DistanceMatrix {
    let packed: Vec<_> = workers.install(|| {
        alignment
            .sequences
            .par_iter()
            .map(|row| {
                let sampled: Vec<_> = row
                    .iter()
                    .zip(weights)
                    .flat_map(|(&base, &n)| std::iter::repeat_n(base, usize::try_from(n).unwrap()))
                    .collect();
                optimized::pack_fast(&sampled)
            })
            .collect()
    });
    counted_matrix(original, &packed, workers, crate::counts)
}
fn counted_matrix(
    original: &DistanceMatrix,
    packed: &[Packed],
    workers: &rayon::ThreadPool,
    counts: impl Fn(&Packed, &Packed) -> (u64, u64, u64) + Sync,
) -> DistanceMatrix {
    let mut result = original.clone();
    // Bounded blocks write the existing triangular arrays directly; there is no
    // pair-by-pattern table or second O(n²) temporary category array.
    workers.install(|| {
        result
            .distances
            .par_chunks_mut(256)
            .zip(result.variances.par_chunks_mut(256))
            .zip(result.shared_sites.par_chunks_mut(256))
            .enumerate()
            .for_each(|(block, ((ds, vs), ss))| {
                let k = block * 256;
                let mut lo = 1;
                let mut hi = packed.len();
                while lo + 1 < hi {
                    let mid = lo + (hi - lo) / 2;
                    if mid * (mid - 1) / 2 <= k {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let mut i = lo;
                let mut j = k - i * (i - 1) / 2;
                for ((d, v), s) in ds.iter_mut().zip(vs).zip(ss) {
                    let (valid, ts, tv) = counts(&packed[i], &packed[j]);
                    *s = valid;
                    (*d, *v) = if valid >= original.min_overlap {
                        estimate(original.model, valid, ts, tv).unwrap_or((f64::NAN, f64::NAN))
                    } else {
                        (f64::NAN, f64::NAN)
                    };
                    j += 1;
                    if j == i {
                        i += 1;
                        j = 0;
                    }
                }
            });
    });
    result
}
fn undefined(matrix: &DistanceMatrix) -> Option<String> {
    for i in 1..matrix.names.len() {
        for j in 0..i {
            let k = crate::index(i, j);
            if !matrix.distances[k].is_finite() {
                let valid = matrix.shared_sites[k];
                let cause = if valid == 0 {
                    "no comparable sites"
                } else if valid < matrix.min_overlap {
                    "below minimum overlap"
                } else {
                    "model saturation"
                };
                return Some(format!(
                    "undefined distance for '{}' / '{}': {cause} ({valid} shared sites)",
                    matrix.names[i], matrix.names[j]
                ));
            }
        }
    }
    None
}
// Node membership is derived once per inferred tree. A canonical unrooted split uses
// the side excluding taxon zero; a rooted clade always uses the descendants.
fn keys(tree: &ParsedTree, names: &[String], rooted: bool) -> Result<Vec<Option<Vec<usize>>>> {
    let ids: BTreeMap<_, _> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let mut descendants = vec![vec![]; tree.nodes.len()];
    let mut keys = vec![None; tree.nodes.len()];
    for node in (0..tree.nodes.len()).rev() {
        let n = &tree.nodes[node];
        let mut members = if n.children.is_empty() {
            vec![
                *ids.get(n.name.as_str())
                    .ok_or("tree contains unknown tip")?,
            ]
        } else {
            n.children
                .iter()
                .flat_map(|&c| descendants[c].iter().copied())
                .collect()
        };
        members.sort_unstable();
        descendants[node].clone_from(&members);
        if node == 0 || n.children.is_empty() {
            continue;
        }
        if !rooted && members.contains(&0) {
            members = (0..names.len())
                .filter(|x| members.binary_search(x).is_err())
                .collect();
        }
        if members.len() > 1
            && (rooted && members.len() < names.len() || !rooted && members.len() < names.len() - 1)
        {
            keys[node] = Some(members);
        }
    }
    Ok(keys)
}
fn annotated(
    tree: &ParsedTree,
    keys: &[Option<Vec<usize>>],
    frequencies: &BTreeMap<Vec<usize>, usize>,
    replicates: usize,
) -> String {
    // Iterative postorder avoids recursion on ladder trees. Preserve the original
    // branch-length values and ordering; only internal support labels are added.
    let mut strings = vec![String::new(); tree.nodes.len()];
    for i in (0..tree.nodes.len()).rev() {
        let node = &tree.nodes[i];
        let mut s = if node.children.is_empty() {
            crate::quote(&node.name)
        } else {
            format!(
                "({})",
                node.children
                    .iter()
                    .map(|&c| strings[c].as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        if let Some(key) = &keys[i] {
            let count = frequencies.get(key).copied().unwrap_or(0);
            write!(s, "{:.6}", 100. * count as f64 / replicates as f64).unwrap();
        }
        if let Some(length) = node.length {
            write!(s, ":{length:.12}").unwrap();
        }
        strings[i] = s;
    }
    format!("{};\n", strings[0])
}

/// Infer the original tree and ordinary bootstrap replicates. Any undefined pair or
/// failed replicate withholds all support. Failed replicates are never replaced.
pub fn bootstrap(
    alignment: &Alignment,
    original: &DistanceMatrix,
    options: &BootstrapOptions,
) -> Result<BootstrapResult> {
    alignment.validate()?;
    original.validate()?;
    if options.replicates == 0 {
        return Err("bootstrap replicate count must be positive".into());
    }
    if !matches!(options.method, Method::Sorted | Method::Upgma) {
        return Err("bootstrap supports only sorted NJ and UPGMA".into());
    }
    if !original.matches(alignment, original.model, original.min_overlap) {
        return Err(
            "bootstrap requires the exact source alignment; a distance cache is insufficient"
                .into(),
        );
    }
    // Conservative crossover measured across n=24..1024 and L=2000..100000.
    // Both kernels give identical integer counts; this changes only runtime.
    let use_weighted =
        original.names.len() < 256 && (original.names.len() < 128 || original.columns > 4096);
    let workers = pool(options.distance_threads)?;
    let start = Instant::now();
    let mut report = BootstrapReport {
        seed: options.seed,
        requested: options.replicates,
        completed: 0,
        attempted: 0,
        method: options.method,
        support_kind: if options.method == Method::Upgma {
            "rooted clades"
        } else {
            "unrooted splits"
        },
        support_scale: "percent",
        support_withheld: true,
        cancelled: false,
        failures: vec![],
        failed_replicates: 0,
        distance_s: 0.,
        tree_s: 0.,
        total_s: 0.,
        algorithm: if use_weighted {
            "exact site multiplicities; packed bitplane counts"
        } else {
            "exact site multiplicities; explicit packed resampling"
        },
        rng: "splitmix64-rejection-v1",
        replicate_trees: None,
    };
    let mut collection = options
        .replicate_trees
        .as_ref()
        .map(|path| CollectionWriter::new(path, options))
        .transpose()?;
    let mut tree = None;
    if let Some(cause) = undefined(original) {
        report.failures.push(format!("original: {cause}"));
    } else {
        let now = Instant::now();
        match original
            .tree(options.method, options.tree_threads)
            .and_then(complete_tree)
        {
            Ok(value) => tree = Some(value),
            Err(error) => report.failures.push(format!("original inference: {error}")),
        }
        report.tree_s += now.elapsed().as_secs_f64();
    }
    let reference = tree
        .as_ref()
        .map(|tree| parse_newick(&tree.trees[0]))
        .transpose()?;
    let reference_keys = reference
        .as_ref()
        .map(|tree| keys(tree, &original.names, options.method == Method::Upgma))
        .transpose()?;
    // Count only original branches. Replicate trees are still inferred freely;
    // retaining every novel replicate split would waste O(B*n²) memory.
    let mut frequencies: BTreeMap<Vec<usize>, usize> = reference_keys
        .iter()
        .flatten()
        .flatten()
        .cloned()
        .map(|key| (key, 0))
        .collect();
    if tree.is_some() {
        let packed: Vec<_> = if use_weighted {
            alignment
                .sequences
                .iter()
                .map(|s| optimized::pack_fast(s))
                .collect()
        } else {
            vec![]
        };
        for replicate in 0..options.replicates {
            if options.cancel_file.as_ref().is_some_and(|p| p.exists()) {
                report.cancelled = true;
                report.failures.push("cancelled between replicates".into());
                break;
            }
            report.attempted += 1;
            let now = Instant::now();
            let weights = multiplicities(original.columns, options.seed, replicate);
            let matrix = if use_weighted {
                weighted_matrix(original, &packed, &weights, &workers)
            } else {
                resampled_matrix(original, alignment, &weights, &workers)
            };
            report.distance_s += now.elapsed().as_secs_f64();
            let now = Instant::now();
            let result = if let Some(cause) = undefined(&matrix) {
                Err(cause.into())
            } else {
                matrix
                    .tree(options.method, options.tree_threads)
                    .and_then(complete_tree)
            };
            match result {
                Ok(replicate_tree) => {
                    let parsed = parse_newick(&replicate_tree.trees[0])?;
                    let splits: BTreeSet<_> =
                        keys(&parsed, &original.names, options.method == Method::Upgma)?
                            .into_iter()
                            .flatten()
                            .collect();
                    for split in splits {
                        if let Some(count) = frequencies.get_mut(&split) {
                            *count += 1;
                        }
                    }
                    if let Some(collection) = &mut collection {
                        collection.tree(
                            replicate,
                            &replicate_tree.trees[0],
                            options.method == Method::Upgma,
                        )?;
                    }
                    report.completed += 1;
                }
                Err(error) => {
                    report.failed_replicates += 1;
                    // Bound diagnostic storage while retaining an exact failure count.
                    if report.failures.len() < 20 {
                        report
                            .failures
                            .push(format!("replicate {}: {error}", replicate + 1));
                    }
                }
            }
            report.tree_s += now.elapsed().as_secs_f64();
            report.total_s = start.elapsed().as_secs_f64();
            if let Some(path) = &options.progress {
                crate::write_json(path, &report)?;
            }
        }
    }
    if report.completed == report.requested && report.failures.is_empty() {
        let original_tree = tree.as_mut().unwrap();
        original_tree.trees[0] = annotated(
            reference.as_ref().unwrap(),
            reference_keys.as_ref().unwrap(),
            &frequencies,
            options.replicates,
        );
        report.support_withheld = false;
    }
    report.total_s = start.elapsed().as_secs_f64();
    if let Some(collection) = collection {
        report.replicate_trees = Some(collection.finish(&report)?);
    }
    if let Some(path) = &options.progress
        && let Err(cause) = crate::write_json(path, &report)
    {
        return Err(Box::new(BootstrapPublicationError { report, cause }));
    }
    Ok(BootstrapResult { tree, report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model;
    fn alignment() -> Alignment {
        Alignment {
            names: ["a 'one", "b", "c", "d", "e"].map(String::from).to_vec(),
            sequences: vec![
                b"AAAAAAAAAACCAAAACCAA".repeat(12),
                b"AAAAAAAAAACCAAAACCAA".repeat(12),
                b"AAAAGGAAAACCAAAACCAA".repeat(12),
                b"AAAAGGAAAACCAAAACCAA".repeat(12),
                b"AAAAAAAAAACCAAGACCAA".repeat(12),
            ],
        }
    }
    fn options(method: Method, threads: usize) -> BootstrapOptions {
        BootstrapOptions {
            replicates: 32,
            seed: 319,
            method,
            distance_threads: threads,
            tree_threads: threads,
            progress: None,
            cancel_file: None,
            replicate_trees: None,
        }
    }
    fn explicit(a: &Alignment, weights: &[u64]) -> Alignment {
        Alignment {
            names: a.names.clone(),
            sequences: a
                .sequences
                .iter()
                .map(|row| {
                    row.iter()
                        .zip(weights)
                        .flat_map(|(&base, &n)| {
                            std::iter::repeat_n(base, usize::try_from(n).unwrap())
                        })
                        .collect()
                })
                .collect(),
        }
    }
    #[test]
    fn retained_trees_match_exact_resampling_without_changing_support() {
        use basecraft_io::newick::parse_trees;
        let dir = tempfile::tempdir().unwrap();
        let a = alignment();
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        for method in [Method::Sorted, Method::Upgma] {
            let mut settings = options(method, 2);
            let baseline = bootstrap(&a, &matrix, &settings).unwrap();
            let path = dir.path().join(format!("{method:?}.nex"));
            settings.replicate_trees = Some(path.clone());
            let result = bootstrap(&a, &matrix, &settings).unwrap();
            assert_eq!(baseline.tree.unwrap().trees, result.tree.unwrap().trees);
            let retained = result.report.replicate_trees.unwrap();
            assert_eq!(retained.path, path);
            assert_eq!(retained.status, "complete");
            assert_eq!(retained.trees, settings.replicates);
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains("status=complete"));
            let trees = parse_trees(&text).unwrap();
            assert_eq!(trees.len(), settings.replicates);
            for (r, tree) in trees.iter().enumerate() {
                assert_eq!(tree.name, format!("replicate_{}", r + 1));
                let weights = multiplicities(matrix.columns, settings.seed, r);
                let sampled = explicit(&a, &weights);
                let expected = DistanceMatrix::calculate(&sampled, Model::P, 1, 1)
                    .unwrap()
                    .tree(method, 1)
                    .unwrap();
                assert_eq!(tree.nodes, parse_newick(&expected.trees[0]).unwrap().nodes);
            }
            assert!(!partial_collection_path(&path).exists());
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn partial_collection_retains_only_actual_successes_with_original_ids() {
        let dir = tempfile::tempdir().unwrap();
        let a = Alignment {
            names: vec!["a".into(), "b".into()],
            sequences: vec![b"A---".to_vec(), b"AAAA".to_vec()],
        };
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        let mut settings = options(Method::Sorted, 2);
        let path = dir.path().join("replicates.nex");
        settings.replicate_trees = Some(path.clone());
        let result = bootstrap(&a, &matrix, &settings).unwrap();
        assert!(result.report.support_withheld);
        assert_eq!(result.report.attempted, settings.replicates);
        let retained = result.report.replicate_trees.unwrap();
        assert_eq!(retained.status, "partial");
        assert!(!path.exists());
        let trees =
            basecraft_io::newick::parse_trees(&std::fs::read_to_string(&retained.path).unwrap())
                .unwrap();
        let mut expected_ids = vec![];
        for r in 0..settings.replicates {
            let sampled = explicit(&a, &multiplicities(matrix.columns, settings.seed, r));
            let sampled_matrix = DistanceMatrix::calculate(&sampled, Model::P, 1, 1).unwrap();
            if undefined(&sampled_matrix).is_none() {
                expected_ids.push(format!("replicate_{}", r + 1));
            }
        }
        assert_eq!(
            trees.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(trees.len(), result.report.completed);
        assert_eq!(
            trees.len() + result.report.failed_replicates,
            settings.replicates
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn cancelled_and_disconnected_collections_are_explicitly_incomplete() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = alignment();
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        let mut settings = options(Method::Upgma, 1);
        settings.replicate_trees = Some(dir.path().join("cancelled.nex"));
        settings.cancel_file = Some(dir.path().join("cancel"));
        std::fs::write(settings.cancel_file.as_ref().unwrap(), "").unwrap();
        let result = bootstrap(&a, &matrix, &settings).unwrap();
        let retained = result.report.replicate_trees.unwrap();
        assert_eq!(retained.status, "cancelled");
        assert_eq!(retained.trees, 0);
        assert!(retained.path.ends_with("cancelled.partial.nex"));
        assert!(
            std::fs::read_to_string(retained.path)
                .unwrap()
                .contains("status=cancelled")
        );
        settings.cancel_file = None;
        settings.replicate_trees = Some(dir.path().join("disconnected.nex"));
        a.names = vec!["a".into(), "b".into()];
        a.sequences = vec![b"AAA---".to_vec(), b"---AAA".to_vec()];
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        let result = bootstrap(&a, &matrix, &settings).unwrap();
        assert!(result.tree.is_none());
        assert_eq!(result.report.attempted, 0);
        assert!(result.report.failures[0].contains("no comparable sites"));
        assert_eq!(
            result.report.replicate_trees.unwrap().status,
            "original_failed"
        );
    }

    #[test]
    fn cancellation_after_progress_retains_exact_successful_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let a = alignment();
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        let mut settings = options(Method::Sorted, 1);
        settings.replicates = 10000;
        settings.progress = Some(dir.path().join("progress.json"));
        settings.cancel_file = Some(dir.path().join("cancel"));
        settings.replicate_trees = Some(dir.path().join("trees.nex"));
        let progress = settings.progress.clone().unwrap();
        let cancel = settings.cancel_file.clone().unwrap();
        let requester = std::thread::spawn(move || {
            let start = Instant::now();
            while !progress.exists() {
                assert!(start.elapsed().as_secs() < 10, "no bootstrap progress");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            std::fs::write(cancel, "").unwrap();
        });
        let result = bootstrap(&a, &matrix, &settings).unwrap();
        requester.join().unwrap();
        assert!(result.report.cancelled);
        assert!(result.report.completed > 0);
        assert!(result.report.completed < settings.replicates);
        assert_eq!(result.report.completed, result.report.attempted);
        let collection = result.report.replicate_trees.unwrap();
        let trees =
            basecraft_io::newick::parse_trees(&std::fs::read_to_string(collection.path).unwrap())
                .unwrap();
        assert_eq!(trees.len(), result.report.completed);
        for (r, tree) in trees.iter().enumerate() {
            assert_eq!(tree.name, format!("replicate_{}", r + 1));
        }
    }

    #[test]
    fn collection_io_errors_preserve_existing_files_and_remove_own_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let a = alignment();
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        let mut settings = options(Method::Sorted, 1);
        let path = dir.path().join("trees.nex");
        settings.replicate_trees = Some(path.clone());
        for collision in [&path, &partial_collection_path(&path)] {
            std::fs::write(collision, "existing").unwrap();
            assert!(bootstrap(&a, &matrix, &settings).is_err());
            assert_eq!(std::fs::read_to_string(collision).unwrap(), "existing");
            std::fs::remove_file(collision).unwrap();
        }
        settings.progress = Some(dir.path().join("missing/progress.json"));
        assert!(bootstrap(&a, &matrix, &settings).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        settings.progress = None;
        settings.replicate_trees = Some(dir.path().join("missing/trees.nex"));
        assert!(bootstrap(&a, &matrix, &settings).is_err());
        // A race after preflight must also be no-clobber.
        let writer = CollectionWriter::new(&path, &settings).unwrap();
        std::fs::write(&path, "racing writer").unwrap();
        let result = bootstrap(&a, &matrix, &options(Method::Sorted, 1)).unwrap();
        assert!(writer.finish(&result.report).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "racing writer");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn exact_integer_counts_and_corrections_match_resampled_packed_engine() {
        let mut a = alignment();
        a.sequences[0][65] = b'N';
        a.sequences[1][66] = b'-';
        a.sequences[2][127] = b'U';
        let packed: Vec<_> = a
            .sequences
            .iter()
            .map(|s| optimized::pack_fast(s))
            .collect();
        for replicate in 0..48 {
            let weights = multiplicities(a.sequences[0].len(), 71, replicate);
            assert_eq!(weights.iter().sum::<u64>(), a.sequences[0].len() as u64);
            let resampled = explicit(&a, &weights);
            let explicit_packed: Vec<_> = resampled
                .sequences
                .iter()
                .map(|s| optimized::pack_fast(s))
                .collect();
            let planes = planes(&weights);
            for i in 1..a.names.len() {
                for j in 0..i {
                    assert_eq!(
                        weighted_counts(&packed[i], &packed[j], &planes),
                        crate::counts(&explicit_packed[i], &explicit_packed[j])
                    );
                }
            }
            for model in [Model::P, Model::Jc69, Model::K2p] {
                let original = DistanceMatrix::calculate(&a, model, 1, 1).unwrap();
                let actual = weighted_matrix(&original, &packed, &weights, &pool(2).unwrap());
                let expected = DistanceMatrix::calculate(&resampled, model, 1, 1).unwrap();
                let fallback = resampled_matrix(&original, &a, &weights, &pool(2).unwrap());
                assert_eq!(actual.distances, fallback.distances);
                assert_eq!(actual.shared_sites, expected.shared_sites);
                assert_eq!(actual.distances, expected.distances);
                assert_eq!(actual.variances, expected.variances);
                for method in [Method::Sorted, Method::Upgma] {
                    assert_eq!(
                        actual.tree(method, 1).unwrap().trees,
                        expected.tree(method, 2).unwrap().trees
                    );
                }
            }
        }
    }
    #[test]
    fn support_preserves_original_lengths_and_is_worker_independent() {
        let a = alignment();
        for method in [Method::Sorted, Method::Upgma] {
            let original = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
            let original_tree = parse_newick(&original.tree(method, 1).unwrap().trees[0]).unwrap();
            let one = bootstrap(&a, &original, &options(method, 1)).unwrap();
            let four = bootstrap(&a, &original, &options(method, 4)).unwrap();
            assert!(!one.report.support_withheld);
            assert_eq!(one.report.completed, 32);
            assert_eq!(one.tree.as_ref().unwrap().trees, four.tree.unwrap().trees);
            let supported = parse_newick(&one.tree.unwrap().trees[0]).unwrap();
            assert_eq!(original_tree.nodes.len(), supported.nodes.len());
            for (original, supported) in original_tree.nodes.iter().zip(&supported.nodes) {
                assert_eq!(original.children, supported.children);
                assert_eq!(original.length, supported.length);
                if original.children.is_empty() {
                    assert_eq!(original.name, supported.name);
                }
            }
            let keys = keys(&supported, &a.names, method == Method::Upgma).unwrap();
            for (i, key) in keys.iter().enumerate() {
                if key.as_deref() == Some(&[2, 3]) {
                    assert_eq!(supported.nodes[i].name, "100.000000");
                }
            }
        }
    }
    #[test]
    fn support_frequencies_match_ordinary_resampling_oracle() {
        let a = Alignment {
            names: ["a", "b", "c", "d", "e"].map(String::from).to_vec(),
            sequences: vec![
                b"AAAAAAAAAA".to_vec(),
                b"AAAAAAAATC".to_vec(),
                b"CCAAAAAACC".to_vec(),
                b"CCAAAAAATC".to_vec(),
                b"ACAAAAAACC".to_vec(),
            ],
        };
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        for method in [Method::Sorted, Method::Upgma] {
            let settings = options(method, 2);
            let result = bootstrap(&a, &matrix, &settings).unwrap();
            let actual = parse_newick(&result.tree.unwrap().trees[0]).unwrap();
            let actual_keys = keys(&actual, &a.names, method == Method::Upgma).unwrap();
            let mut frequencies = BTreeMap::<Vec<usize>, usize>::new();
            for replicate in 0..settings.replicates {
                let weights = multiplicities(matrix.columns, settings.seed, replicate);
                let sampled = explicit(&a, &weights);
                let matrix = DistanceMatrix::calculate(&sampled, Model::P, 1, 1).unwrap();
                let tree = parse_newick(&matrix.tree(method, 1).unwrap().trees[0]).unwrap();
                let unique: BTreeSet<_> = keys(&tree, &a.names, method == Method::Upgma)
                    .unwrap()
                    .into_iter()
                    .flatten()
                    .collect();
                for split in unique {
                    *frequencies.entry(split).or_default() += 1;
                }
            }
            let mut nontrivial = false;
            for (i, key) in actual_keys.iter().enumerate() {
                if let Some(key) = key {
                    let expected = 100. * frequencies.get(key).copied().unwrap_or(0) as f64
                        / settings.replicates as f64;
                    let actual = actual.nodes[i].name.parse::<f64>().unwrap();
                    assert!((actual - expected).abs() < 0.000_001);
                    nontrivial |= actual > 0. && actual < 100.;
                }
            }
            assert!(nontrivial, "oracle must exercise fractional support");
        }
    }
    #[test]
    fn explicit_packed_fallback_is_worker_independent() {
        let mut a = alignment();
        let rows = a.sequences.clone();
        a.names = (0..128).map(|i| format!("tip{i}")).collect();
        a.sequences = (0..128).map(|i| rows[i % rows.len()].clone()).collect();
        let matrix = DistanceMatrix::calculate(&a, Model::K2p, 1, 1).unwrap();
        for method in [Method::Sorted, Method::Upgma] {
            let mut one = options(method, 1);
            one.replicates = 4;
            let mut four = options(method, 4);
            four.replicates = 4;
            let result_one = bootstrap(&a, &matrix, &one).unwrap();
            let result_four = bootstrap(&a, &matrix, &four).unwrap();
            assert!(
                result_one
                    .report
                    .algorithm
                    .contains("explicit packed resampling")
            );
            assert_eq!(
                result_one.tree.unwrap().trees,
                result_four.tree.unwrap().trees
            );
        }
    }
    #[test]
    fn rooted_clades_and_unrooted_splits_distinguish_rerootings() {
        let names = ["a", "b", "c", "d", "e"].map(String::from);
        let first = parse_newick("((a,b),(c,(d,e)));").unwrap();
        let rerooted = parse_newick("(a,(b,(c,(d,e))));").unwrap();
        let sets = |t, rooted| {
            keys(t, &names, rooted)
                .unwrap()
                .into_iter()
                .flatten()
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(sets(&first, false), sets(&rerooted, false));
        assert_ne!(sets(&first, true), sets(&rerooted, true));
        assert_eq!(sets(&first, false).len(), 2);
    }
    #[test]
    fn failed_replicates_are_not_skipped_or_replaced() {
        let a = Alignment {
            names: vec!["a".into(), "b".into()],
            sequences: vec![b"A---".to_vec(), b"AAAA".to_vec()],
        };
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        let result = bootstrap(&a, &matrix, &options(Method::Sorted, 2)).unwrap();
        assert!(result.report.support_withheld);
        assert_eq!(result.report.attempted, 32);
        assert_eq!(
            result.report.completed + result.report.failed_replicates,
            32
        );
        assert!(result.report.failed_replicates > 0);
        assert!(
            result
                .report
                .failures
                .iter()
                .any(|s| s.contains("no comparable sites"))
        );
        assert_eq!(
            result.tree.unwrap().trees,
            matrix.tree(Method::Sorted, 1).unwrap().trees
        );
    }
    #[test]
    fn original_errors_saturation_overlap_cache_and_cancellation_are_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Alignment {
            names: vec!["a".into(), "b".into()],
            sequences: vec![b"AAAA".to_vec(), b"GGGG".to_vec()],
        };
        let saturated = DistanceMatrix::calculate(&a, Model::K2p, 1, 1).unwrap();
        let failed = bootstrap(&a, &saturated, &options(Method::Sorted, 1)).unwrap();
        assert!(failed.tree.is_none());
        assert_eq!(failed.report.attempted, 0);
        assert!(failed.report.failures[0].contains("saturation"));
        a.sequences[1] = b"AA--".to_vec();
        let short = DistanceMatrix::calculate(&a, Model::P, 3, 1).unwrap();
        let failed = bootstrap(&a, &short, &options(Method::Upgma, 1)).unwrap();
        assert!(failed.report.failures[0].contains("minimum overlap"));
        let matrix = DistanceMatrix::calculate(&a, Model::P, 1, 1).unwrap();
        let mut other = a.clone();
        other.sequences[0][0] = b'T';
        assert!(bootstrap(&other, &matrix, &options(Method::Sorted, 1)).is_err());
        assert!(bootstrap(&a, &matrix, &options(Method::Njstar, 1)).is_err());
        let mut settings = options(Method::Upgma, 1);
        settings.progress = Some(dir.path().join("progress.json"));
        settings.cancel_file = Some(dir.path().join("cancel"));
        std::fs::write(settings.cancel_file.as_ref().unwrap(), "").unwrap();
        let cancelled = bootstrap(&a, &matrix, &settings).unwrap();
        assert!(cancelled.report.cancelled);
        assert!(cancelled.report.support_withheld);
        assert_eq!(cancelled.report.completed, 0);
        let progress: serde_json::Value =
            serde_json::from_slice(&std::fs::read(settings.progress.unwrap()).unwrap()).unwrap();
        assert_eq!(progress["cancelled"], true);
    }
    #[test]
    #[ignore = "release benchmark; run with --release --ignored --nocapture"]
    fn benchmark_weighted_against_explicit_packed() {
        for (n, width, repeated) in [
            (24, 2000, true),
            (24, 2000, false),
            (128, 2000, false),
            (128, 10000, false),
            (256, 2000, false),
            (256, 10000, false),
            (256, 100_000, false),
            (512, 2000, false),
            (1024, 2000, false),
            (128, 100_000, false),
        ] {
            let mut state = 71;
            let a = Alignment {
                names: (0..n).map(|i| format!("t{i}")).collect(),
                sequences: (0..n)
                    .map(|i| {
                        (0..width)
                            .map(|column| {
                                let r = if repeated {
                                    (i + column % 17) as u64
                                } else {
                                    next(&mut state)
                                };
                                if r % 37 == 0 {
                                    b'-'
                                } else if r % 11 == 0 {
                                    b'G'
                                } else {
                                    b'A'
                                }
                            })
                            .collect()
                    })
                    .collect(),
            };
            let original = DistanceMatrix::calculate(&a, Model::P, 1, 4).unwrap();
            let packed: Vec<_> = a
                .sequences
                .iter()
                .map(|s| optimized::pack_fast(s))
                .collect();
            let workers = pool(4).unwrap();
            let mut weighted_s = 0.;
            let mut fallback_s = 0.;
            let mut explicit_s = 0.;
            for replicate in 0..16 {
                let weights = multiplicities(width, 71, replicate);
                let start = Instant::now();
                let actual = weighted_matrix(&original, &packed, &weights, &workers);
                weighted_s += start.elapsed().as_secs_f64();
                let start = Instant::now();
                let fallback = resampled_matrix(&original, &a, &weights, &workers);
                fallback_s += start.elapsed().as_secs_f64();
                assert_eq!(actual.distances, fallback.distances);
                let start = Instant::now();
                let sampled = explicit(&a, &weights);
                let expected = DistanceMatrix::calculate(&sampled, Model::P, 1, 4).unwrap();
                explicit_s += start.elapsed().as_secs_f64();
                assert_eq!(actual.distances, expected.distances);
            }
            println!(
                "n={n} width={width} repeated={repeated} replicates=16 weighted_s={weighted_s:.6} fallback_s={fallback_s:.6} explicit_packed_s={explicit_s:.6}"
            );
        }
    }
}
