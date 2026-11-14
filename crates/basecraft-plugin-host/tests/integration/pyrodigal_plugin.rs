//! End to end: gene prediction with the Pyrodigal plugin on the Pyropia
//! mitochondrion (41.7 kb, genetic code 4). Skips when Pyrodigal isn't
//! installed.

use std::path::Path;

use basecraft_ops::Session;
use basecraft_plugin_host::process::python_interpreter;
use basecraft_plugin_host::{Manifest, PluginProcess, Snapshot};
use basecraft_protocol::{Op, Strand};
use serde_json::json;

fn plugin() -> Option<PluginProcess> {
    let python = python_interpreter();
    let ok = std::process::Command::new(&python)
        .args(["-c", "import pyarrow, numpy, pyrodigal"])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipping: {} lacks pyrodigal", python.display());
        return None;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/pyrodigal");
    let manifest = Manifest::load(&dir).expect("manifest");
    Some(PluginProcess::spawn_with(&manifest, &python).expect("spawn"))
}

/// An empty run for sequence operations, which allocate no files.
fn context() -> basecraft_protocol::operation::InvocationContext {
    basecraft_protocol::operation::InvocationContext {
        schema: 2,
        operation: "test".into(),
        run_id: "test".into(),
        run_root: std::env::temp_dir().display().to_string(),
        inputs: std::collections::BTreeMap::new(),
        outputs: std::collections::BTreeMap::new(),
        cpu_threads: 1,
        cancel_file: "cancel".into(),
        progress_file: "progress".into(),
        input_evidence: std::collections::BTreeMap::new(),
        alphabets: std::collections::BTreeMap::new(),
    }
}

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/pyropia-mito.fasta")
}

#[test]
fn predicts_genes_with_the_mitochondrial_code() {
    crate::isolated_profile::isolate!();
    let Some(mut p) = plugin() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&fixture()).unwrap();
    let snap = Snapshot::of(session.doc_key(doc).unwrap(), session.doc(doc).unwrap()).unwrap();

    let result = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": 4 }),
            context(),
            Some(&snap),
        )
        .unwrap();
    // The benchmark found 52 genes against 31 annotated CDS.
    assert!(
        (40..70).contains(&result.ops.len()),
        "{} genes: {:?}",
        result.ops.len(),
        result.message
    );
    // nad1, GenBank complement(12542..13528), with its annotated start.
    let nad1 = result.ops.iter().any(|op| {
        let Op::FeatureAdd { feature, .. } = op else {
            return false;
        };
        let s = &feature.location.segments[0];
        feature.kind == "CDS"
            && feature.qualifier("transl_table") == Some("4")
            && (s.start, s.end, s.strand) == (12541, 13528, Strand::Reverse)
    });
    assert!(nad1, "nad1 not predicted");
    // Every gene records the code and the frame it was read in.
    for op in &result.ops {
        let Op::FeatureAdd { feature, .. } = op else {
            panic!("not a feature.add")
        };
        assert_eq!(feature.qualifier("transl_table"), Some("4"));
        assert!(
            matches!(feature.qualifier("codon_start"), Some("1" | "2" | "3")),
            "{feature:?}"
        );
    }
    // Without a code, nothing is predicted.
    let missing = p
        .invoke("predict_genes", json!({}), context(), Some(&snap))
        .unwrap_err();
    assert!(
        missing.to_string().contains("needs a genetic code"),
        "{missing}"
    );

    // Within a range, every gene stays inside it.
    let result = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": 4, "start": 10_000, "end": 20_000 }),
            context(),
            Some(&snap),
        )
        .unwrap();
    assert!(result.ops.len() > 1, "{:?}", result.message);
    for op in &result.ops {
        let Op::FeatureAdd { feature, .. } = op else {
            panic!("not a feature.add")
        };
        let s = &feature.location.segments[0];
        assert!(s.start >= 10_000 && s.end <= 20_000, "{s:?}");
    }
}

#[test]
fn short_sequences_need_code_4_or_11() {
    crate::isolated_profile::isolate!();
    let Some(mut p) = plugin() else { return };
    let dir = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(fixture()).unwrap();
    let seq: String = text.lines().skip(1).collect();
    let short = dir.path().join("short.fasta");
    std::fs::write(&short, format!(">short\n{}\n", &seq[..15_000])).unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&short).unwrap();
    let snap = Snapshot::of(session.doc_key(doc).unwrap(), session.doc(doc).unwrap()).unwrap();

    let err = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": 2 }),
            context(),
            Some(&snap),
        )
        .unwrap_err();
    assert!(err.to_string().contains("code 4 or 11"), "{err}");
    let result = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": 4 }),
            context(),
            Some(&snap),
        )
        .unwrap();
    assert!(result.ops.len() > 1, "{:?}", result.message);
}

/// A predicted gene with its start, end, strand and whether its left edge
/// is cut off.
type Gene = (basecraft_protocol::Feature, (u64, u64, Strand, bool));

/// Each predicted gene.
fn genes(p: &mut PluginProcess, snap: &Snapshot, table: u8) -> Vec<Gene> {
    let result = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": table }),
            context(),
            Some(snap),
        )
        .unwrap();
    result
        .ops
        .into_iter()
        .map(|op| {
            let Op::FeatureAdd { feature, .. } = op else {
                panic!("not a feature.add")
            };
            let s = feature.location.segments[0].clone();
            let span = (s.start, s.end, s.strand, s.partial_start);
            (feature, span)
        })
        .collect()
}

fn reverse_complement(codon: &[u8]) -> Vec<u8> {
    codon
        .iter()
        .rev()
        .map(|b| match b {
            b'A' => b'T',
            b'C' => b'G',
            b'G' => b'C',
            b'T' => b'A',
            other => *other,
        })
        .collect()
}

/// Prodigal's start rule follows the table: under 11 (bacteria, archaea,
/// plastids) GTG and TTG start genes; under the standard code only ATG.
#[test]
fn table_11_starts_genes_at_gtg_and_ttg_and_table_1_does_not() {
    crate::isolated_profile::isolate!();
    let Some(mut p) = plugin() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&fixture()).unwrap();
    let snap = Snapshot::of(session.doc_key(doc).unwrap(), session.doc(doc).unwrap()).unwrap();
    let sequence = session.doc(doc).unwrap();
    let starts = |found: &[Gene]| {
        let mut codons = std::collections::BTreeSet::new();
        for (feature, (start, end, strand, _)) in found {
            let segment = &feature.location.segments[0];
            let complete_start = if *strand == Strand::Forward {
                !segment.partial_start
            } else {
                !segment.partial_end
            };
            if complete_start {
                let codon = if *strand == Strand::Forward {
                    sequence.read(*start, start + 3)
                } else {
                    reverse_complement(&sequence.read(end - 3, *end))
                };
                codons.insert(String::from_utf8(codon.to_ascii_uppercase()).unwrap());
            }
        }
        codons
    };
    let standard = starts(&genes(&mut p, &snap, 1));
    let bacterial = starts(&genes(&mut p, &snap, 11));
    assert_eq!(
        standard,
        std::collections::BTreeSet::from(["ATG".to_owned()]),
        "table 1"
    );
    assert!(
        bacterial.contains("GTG") || bacterial.contains("TTG"),
        "table 11: {bacterial:?}"
    );
}

/// A gene cut off at the 5' edge, in a sequence whose length is not a whole
/// number of codons, still spans whole codons from its first base:
/// `codon_start` 1.
#[test]
fn a_gene_cut_at_the_5_prime_edge_starts_at_a_whole_codon() {
    crate::isolated_profile::isolate!();
    let Some(mut p) = plugin() else { return };
    let dir = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(fixture()).unwrap();
    let seq: String = text.lines().skip(1).collect();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&fixture()).unwrap();
    let snap = Snapshot::of(session.doc_key(doc).unwrap(), session.doc(doc).unwrap()).unwrap();
    // A forward gene in the first half, cut 100 bases plus 0, 1 or 2 inside.
    let (_, (start, _, _, _)) = genes(&mut p, &snap, 4)
        .into_iter()
        .find(|(_, (start, end, strand, partial))| {
            *strand == Strand::Forward
                && !partial
                && *start > 1_000
                && *start < 15_000
                && end - start > 600
        })
        .expect("a forward gene in the first half");
    for shift in 0..3 {
        let cut = usize::try_from(start).unwrap() + 100 + shift;
        let mut rest = seq[cut..].to_owned();
        if rest.len().is_multiple_of(3) {
            rest.pop();
        }
        let path = dir.path().join(format!("cut{shift}.fasta"));
        std::fs::write(&path, format!(">cut\n{rest}\n")).unwrap();
        let cut_doc = session.open(&path).unwrap();
        let cut_snap = Snapshot::of(
            session.doc_key(cut_doc).unwrap(),
            session.doc(cut_doc).unwrap(),
        )
        .unwrap();
        let found = genes(&mut p, &cut_snap, 4);
        assert!(
            found.iter().any(
                |(_, (start, _, strand, partial))| *strand == Strand::Forward
                    && *partial
                    && *start < 3
            ),
            "shift {shift}: no gene runs off the 5' edge"
        );
        for (feature, (start, end, _, _)) in &found {
            assert_eq!((end - start) % 3, 0, "shift {shift}: {feature:?}");
            assert_eq!(feature.qualifier("codon_start"), Some("1"));
        }
    }
}
