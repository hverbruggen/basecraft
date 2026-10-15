//! Always-on compact fixture consumers: no external cache or optional download.
use basecraft_io::{abif, fasta::FastaIndex, genbank, gff3, newick, spart};
use basecraft_protocol::Strand;
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact")
            .join(name),
    )
    .unwrap()
}
fn text(name: &str) -> String {
    String::from_utf8(fixture(name)).unwrap()
}

#[test]
fn sequence_annotations_have_independent_coordinates_and_roundtrip() {
    let gb = genbank::parse(&fixture("sequences-annotations/locus.gb")).unwrap();
    let rec = &gb[0];
    assert_eq!(rec.sequence.len(), 421);
    let fa = fixture("sequences-annotations/locus.fasta");
    let index = FastaIndex::build(&fa).unwrap();
    assert_eq!(index.records[0].read(&fa, 0, 421).unwrap(), rec.sequence);
    let gff = gff3::parse(&fixture("sequences-annotations/locus.gff3")).unwrap();
    let features = &gff.sequences[0].features;
    assert_eq!(features.len(), 2);
    assert_eq!(features[0].qualifier(gff3::PHASE), Some("0"));
    for (feature, expected) in features.iter().zip([
        vec![(0, 61, Strand::Forward), (90, 209, Strand::Forward)],
        vec![(241, 421, Strand::Reverse)],
    ]) {
        let coordinates: Vec<_> = feature
            .location
            .segments
            .iter()
            .map(|s| (s.start, s.end, s.strand))
            .collect();
        assert_eq!(coordinates, expected);
    }
    for (a, b) in rec.features.iter().zip(features) {
        assert_eq!(a.location, b.location);
    }
    let mut out = Vec::new();
    genbank::write(&mut out, rec).unwrap();
    assert_eq!(genbank::parse(&out).unwrap(), gb);
    out.clear();
    gff3::write(&mut out, &[(&gff.sequences[0], 421)]).unwrap();
    assert!(String::from_utf8_lossy(&out).contains("\t91\t209\t.\t+\t2\t"));
    assert_eq!(gff3::parse(&out).unwrap().sequences, gff.sequences);
    for (file, length) in [
        ("coding.fasta", 180),
        ("protein.fasta", 59),
        ("noncoding.fasta", 32),
    ] {
        let bytes = fixture(&format!("sequences-annotations/{file}"));
        assert_eq!(FastaIndex::build(&bytes).unwrap().records[0].len, length);
    }
}

#[test]
fn fasta_preserves_duplicate_unicode_ids_wrapping_and_crlf() {
    let bytes = fixture("alignment-editing/identifiers.fasta");
    let index = FastaIndex::build(&bytes).unwrap();
    assert_eq!(index.records.len(), 2);
    for record in &index.records {
        assert_eq!(record.name, "sample_α");
        assert_eq!(record.len, 180);
        assert_eq!(record.read(&bytes, 0, 3).unwrap(), b"ATG");
    }
    assert!(FastaIndex::build(&fixture("recovery-errors/invalid/missing-header.fasta")).is_err());
    assert_eq!(
        FastaIndex::build(&fixture("recovery-errors/invalid/empty.fasta"))
            .unwrap()
            .records
            .len(),
        0
    );
}

#[test]
fn tree_and_spart_preserve_specimen_mapping_and_partial_membership() {
    let work = spart::parse_text(&text("species-delimitation/hypotheses.spart")).unwrap();
    assert_eq!(work.specimens.len(), 8);
    assert_eq!(work.hypotheses.len(), 2);
    assert_eq!(work.hypotheses[1].unassigned.len(), 1);
    assert!(work.hypotheses[1].unassigned.contains("specimen_08"));
    let expected: std::collections::BTreeSet<_> =
        (1..=8).map(|i| format!("specimen_{i:02}")).collect();
    for file in ["time.nwk", "time-binary.nwk", "substitutions.nwk"] {
        let tree = newick::parse_newick(&text(&format!("species-delimitation/{file}"))).unwrap();
        let names = tree
            .nodes
            .iter()
            .filter(|n| n.children.is_empty())
            .map(|n| n.name.clone())
            .collect();
        assert_eq!(expected, names);
        if file != "substitutions.nwk" {
            for (i, node) in tree
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.children.is_empty())
            {
                let mut distance = node.length.unwrap();
                let mut parent = tree.nodes[i].parent;
                while let Some(p) = parent {
                    distance += tree.nodes[p].length.unwrap_or(0.0);
                    parent = tree.nodes[p].parent;
                }
                assert!((distance - 1.0).abs() < 1e-12);
            }
        }
    }
    let (out, _) = spart::write_text(&work);
    let reparsed = spart::parse_text(&out).unwrap();
    assert_eq!(reparsed.specimens, work.specimens);
    assert_eq!(
        reparsed.hypotheses[1].unassigned,
        work.hypotheses[1].unassigned
    );
    let nexus = newick::parse_trees(&text("tree-inference/quoted.nex")).unwrap();
    assert!(nexus[0].nodes.iter().any(|n| n.name == "sample α"));
    assert!(nexus[0].nodes.iter().any(|n| n.name == "95"));
}

#[test]
fn synthetic_trace_calls_channels_quality_and_bounds() {
    let trace = abif::parse(&fixture("sanger-review/forward.ab1")).unwrap();
    assert_eq!(trace.samples(), 444);
    assert_eq!(trace.channel_order, *b"GATC");
    let calls = trace.display_calls();
    assert_eq!(calls.bases.len(), 36);
    assert_eq!(calls.positions[0], 12);
    assert_eq!(calls.positions[35], 432);
    assert_eq!(calls.qualities.as_ref().unwrap()[0], 8);
    assert_eq!(calls.qualities.as_ref().unwrap()[12], 35);
    assert_eq!(trace.channels[0][12], 240); // First call A; reordered GATC -> ACGT.
    assert_eq!(calls.bases[12], b'C');
    let reverse = abif::parse(&fixture("sanger-review/reverse.ab1")).unwrap();
    assert_eq!(reverse.display_calls().bases[15], b'N');
    assert_eq!(reverse.display_calls().qualities.as_ref().unwrap()[15], 8);
    assert!(
        abif::parse(&fixture("sanger-review/no-quality.ab1"))
            .unwrap()
            .display_calls()
            .qualities
            .is_none()
    );
    for name in ["truncated.ab1", "peak-outside.ab1"] {
        assert!(abif::parse(&fixture(&format!("recovery-errors/invalid/{name}"))).is_err());
    }
}

#[test]
fn malformed_formats_return_errors() {
    assert!(genbank::parse(&fixture("recovery-errors/invalid/malformed.gb")).is_err());
    assert!(gff3::parse(&fixture("recovery-errors/invalid/malformed.gff3")).is_err());
    assert!(newick::parse_newick(&text("recovery-errors/invalid/malformed.nwk")).is_err());
    assert!(spart::parse_text(&text("recovery-errors/invalid/malformed.spart")).is_err());
}
