//! Native ASAP against the reference command-line program (fixture README).
#![cfg(all(target_os = "linux", target_env = "gnu"))]
use basecraft_asap as asap;
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
    // The reference omits specimens that are singletons at zero distance.
    assert_eq!(run.diagnostics.len(), 1, "{:?}", run.diagnostics);
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
