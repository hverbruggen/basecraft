use basecraft_distance::DistanceMatrixExt;
use basecraft_distance::{Alignment, DistanceMatrix, Model};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/compact")
        .join(name)
}

#[test]
fn compact_known_p_distances_and_realistic_identity() {
    let alignment = Alignment::read(&fixture("tree-inference/known-distances.afa")).unwrap();
    let matrix = DistanceMatrix::calculate(&alignment, Model::P, 1, 1).unwrap();
    for (a, b, expected) in [(0, 1, 1.0 / 12.0), (0, 2, 2.0 / 12.0), (1, 2, 1.0 / 12.0)] {
        assert!((matrix.get(a, b).unwrap() - expected).abs() < 1e-12);
    }
    let alignment = Alignment::read(&fixture("alignment-editing/homologues.afa")).unwrap();
    let matrix = DistanceMatrix::calculate(&alignment, Model::P, 1, 1).unwrap();
    assert_eq!(alignment.names.len(), 8);
    assert!(matrix.get(0, 1).unwrap().abs() < 1e-12);
    assert!(matrix.get(6, 7).unwrap().abs() < 1e-12);
    assert!(matrix.get(0, 2).unwrap() > matrix.get(2, 3).unwrap());
}

#[test]
fn compact_invalid_alignments_are_rejected_without_renaming() {
    for file in [
        "recovery-errors/invalid/ragged.afa",
        "recovery-errors/invalid/empty.fasta",
        "recovery-errors/invalid/symbols.fasta",
        "alignment-editing/identifiers.fasta",
    ] {
        assert!(Alignment::read(&fixture(file)).is_err(), "{file}");
    }
    let alignment = Alignment::read(&fixture("alignment-editing/ambiguous-gappy.afa")).unwrap();
    let matrix = DistanceMatrix::calculate(&alignment, Model::P, 1, 1).unwrap();
    assert_eq!(matrix.names, alignment.names);
    assert!(matrix.distances.iter().all(|d| d.is_finite()));
}

#[test]
fn current_binary_cache_keeps_exact_bytes() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/producers-v1");
    let source = root.join("distance-cache.bin");
    let header: basecraft_protocol::distance_records::CacheHeader =
        serde_json::from_slice(&std::fs::read(root.join("distance-cache-header.json")).unwrap())
            .unwrap();
    let matrix = DistanceMatrix::read(&source).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let written = temp.path().join("cache.bin");
    matrix
        .write_identified(&written, &header.basecraft_identity)
        .unwrap();
    assert_eq!(
        std::fs::read(written).unwrap(),
        std::fs::read(source).unwrap()
    );
}
