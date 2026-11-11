//! Native ASAP against the reference command-line program (fixture README).
#![cfg(all(target_os = "linux", target_env = "gnu"))]
use basecraft_asap as asap;
use basecraft_distance::DistanceMatrixExt;
use std::path::Path;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/fixtures/asap");

fn numbers(text: &str) -> Vec<Vec<f64>> {
    text.lines()
        .skip(1)
        .map(|line| line.split('\t').map(|v| v.parse().unwrap()).collect())
        .collect()
}

fn check(values: &[f64], prefix: &str) {
    let n = 19;
    let settings = asap::Settings {
        sequence_length: 478,
        seed: 1,
        ..asap::Settings::default()
    };
    let run = asap::analyse(&asap::Distances { n, values }, &settings, |_, _| true).unwrap();
    let read = |suffix: &str| {
        std::fs::read_to_string(Path::new(FIXTURE).join(format!("{prefix}.{suffix}"))).unwrap()
    };
    assert_eq!(
        numbers(&run.exact_table()),
        numbers(&read("partitions.tsv"))
    );
    assert_eq!(numbers(&run.node_table(n)), numbers(&read("nodes.tsv")));
    check_subsets(&run, n);
    let zero = run.partitions.iter().find(|p| p.distance == 0.0).unwrap();
    let singletons = zero
        .subsets_detail
        .as_ref()
        .unwrap()
        .iter()
        .filter(|s| s.members.len() == 1)
        .count();
    assert_eq!(
        singletons, 19,
        "zero-distance candidate is the pre-merge partition"
    );
    // The reference omits specimens that are singletons at zero distance.
    assert_eq!(run.diagnostics.len(), 1, "{:?}", run.diagnostics);
}

// Independently recover every node's leaf set, then match scores by membership
// rather than the reference XML's positional score order.
fn check_subsets(run: &asap::Analysis, n: usize) {
    let mut leaf_sets: Vec<Vec<usize>> = Vec::new();
    for (i, node) in run.nodes.iter().enumerate() {
        let mut members = if i < n {
            vec![i]
        } else {
            node.children
                .iter()
                .flat_map(|&c| leaf_sets[c as usize].clone())
                .collect()
        };
        members.sort_unstable();
        leaf_sets.push(members);
    }
    for partition in &run.partitions {
        let subsets = partition.subsets_detail.as_ref().unwrap();
        assert_eq!(subsets.len(), partition.subsets);
        let mut coverage: Vec<_> = subsets.iter().flat_map(|s| s.members.clone()).collect();
        coverage.sort_unstable();
        assert_eq!(coverage, (0..n).collect::<Vec<_>>());
        for subset in subsets {
            let mut members = subset.members.clone();
            members.sort_unstable();
            let node = leaf_sets.iter().position(|s| *s == members).unwrap();
            assert_eq!(subset.p_value, run.nodes[node].p_value);
        }
    }
}

#[test]
fn real_matrix_parallel_results_include_thread_invariant_subsets() {
    let alignment =
        basecraft_distance::Alignment::read(&Path::new(FIXTURE).join("mantidactylus.fasta"))
            .unwrap();
    let matrix = basecraft_distance::DistanceMatrix::calculate(
        &alignment,
        basecraft_distance::Model::Jc69,
        1,
        1,
    )
    .unwrap();
    let distances = asap::Distances {
        n: 19,
        values: &matrix.distances,
    };
    // Exercise early stopping, a partial block and a multi-block budget.
    for cap in [100, 501, 2_003] {
        for seed in [1, 17] {
            let settings = asap::Settings {
                sequence_length: 478,
                seed,
                max_simulations: cap,
                stream: asap::Stream::Parallel,
                ..asap::Settings::default()
            };
            let run = |threads| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap()
                    .install(|| asap::analyse(&distances, &settings, |_, _| true).unwrap())
            };
            let one = run(1);
            let four = run(4);
            assert_eq!(
                serde_json::to_value(&one).unwrap(),
                serde_json::to_value(&four).unwrap()
            );
            check_subsets(&one, 19);
        }
    }
}

#[test]
fn reference_distances_reproduce_reference_results() {
    let text = std::fs::read_to_string(Path::new(FIXTURE).join("mantidactylus.reference-jc69.phy"))
        .unwrap();
    let mut words = text.split_whitespace();
    let n: usize = words.next().unwrap().parse().unwrap();
    let mut full = Vec::new();
    for _ in 0..n {
        words.next();
        full.extend((0..n).map(|_| words.next().unwrap().parse::<f64>().unwrap()));
    }
    let condensed: Vec<f64> = (1..n)
        .flat_map(|i| (0..i).map(move |j| (i, j)))
        .map(|(i, j)| full[j * n + i])
        .collect();
    check(&condensed, "reference-distances");
}

#[test]
fn basecraft_distances_reproduce_reference_results() {
    let alignment =
        basecraft_distance::Alignment::read(&Path::new(FIXTURE).join("mantidactylus.fasta"))
            .unwrap();
    assert_eq!(alignment.sequences[0].len(), 478);
    let matrix = basecraft_distance::DistanceMatrix::calculate(
        &alignment,
        basecraft_distance::Model::Jc69,
        1,
        1,
    )
    .unwrap();
    let values: Vec<f64> = matrix.distances.iter().map(|d| d + 0.0).collect();
    check(&values, "basecraft-distances");
}
