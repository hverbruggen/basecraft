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

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/pyropia-mito.fasta")
}

#[test]
fn predicts_genes_with_the_mitochondrial_code() {
    let Some(mut p) = plugin() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&fixture()).unwrap();
    let snap = Snapshot::of(session.doc_key(doc), session.doc(doc).unwrap()).unwrap();

    let result = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": 4 }),
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
        let Op::FeatureAdd { feature } = op else {
            return false;
        };
        let s = &feature.location.segments[0];
        feature.kind == "CDS"
            && feature.qualifier("transl_table") == Some("4")
            && (s.start, s.end, s.strand) == (12541, 13528, Strand::Reverse)
    });
    assert!(nad1, "nad1 not predicted");

    // Within a range, every gene stays inside it.
    let result = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": 4, "start": 10_000, "end": 20_000 }),
            Some(&snap),
        )
        .unwrap();
    assert!(result.ops.len() > 1, "{:?}", result.message);
    for op in &result.ops {
        let Op::FeatureAdd { feature } = op else {
            panic!("not a feature.add")
        };
        let s = &feature.location.segments[0];
        assert!(s.start >= 10_000 && s.end <= 20_000, "{s:?}");
    }
}

#[test]
fn short_sequences_need_code_4_or_11() {
    let Some(mut p) = plugin() else { return };
    let dir = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(fixture()).unwrap();
    let seq: String = text.lines().skip(1).collect();
    let short = dir.path().join("short.fasta");
    std::fs::write(&short, format!(">short\n{}\n", &seq[..15_000])).unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&short).unwrap();
    let snap = Snapshot::of(session.doc_key(doc), session.doc(doc).unwrap()).unwrap();

    let err = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": 2 }),
            Some(&snap),
        )
        .unwrap_err();
    assert!(err.to_string().contains("code 4 or 11"), "{err}");
    let result = p
        .invoke(
            "predict_genes",
            json!({ "translation_table": 4 }),
            Some(&snap),
        )
        .unwrap();
    assert!(result.ops.len() > 1, "{:?}", result.message);
}
