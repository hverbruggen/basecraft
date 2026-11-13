//! End to end: the ORF finder plugin in a real Python process.
//! Skips when no Python with pyarrow and numpy is available.

#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::path::Path;
use std::time::{Duration, Instant};

use basecraft_ops::Session;
use basecraft_plugin_host::apply::apply_proposals;
use basecraft_plugin_host::process::python_interpreter;
use basecraft_plugin_host::{Manifest, PluginProcess, Snapshot};
use basecraft_protocol::{Actor, Op};
use serde_json::json;

fn plugin() -> Option<PluginProcess> {
    let python = python_interpreter();
    let ok = std::process::Command::new(&python)
        .args(["-c", "import pyarrow, numpy"])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipping: {} lacks pyarrow or numpy", python.display());
        return None;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/orf-finder");
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

/// A GenBank record: poly-C background with one forward ORF of 121 codons
/// (ATG + 120 × AAA + TAA) at 0-based position 10.
fn record(dir: &Path) -> std::path::PathBuf {
    let mut seq = "C".repeat(10);
    seq.push_str("ATG");
    seq.push_str(&"AAA".repeat(120));
    seq.push_str("TAA");
    seq.push_str(&"C".repeat(200));
    let rec = basecraft_io::genbank::Record {
        name: "orftest".into(),
        molecule: "DNA".into(),
        alphabet: None,
        circular: false,
        division: "SYN".into(),
        date: "10-OCT-2026".into(),
        header: Vec::new(),
        features: Vec::new(),
        sequence: seq.into_bytes(),
    };
    let path = dir.join("orf.gb");
    basecraft_io::write_atomic(&path, |w| basecraft_io::genbank::write(w, &rec)).unwrap();
    path
}

#[test]
fn finds_orfs_and_maps_them_onto_a_newer_version() {
    crate::isolated_profile::isolate!();
    let Some(mut p) = plugin() else { return };
    assert_eq!(p.info.operations, vec!["find_orfs", "selftest"]);
    eprintln!("plugin startup: {:?}", p.startup);

    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&record(dir.path())).unwrap();
    let snap = Snapshot::of(session.doc_key(doc).unwrap(), session.doc(doc).unwrap()).unwrap();

    let result = p
        .invoke(
            "find_orfs",
            json!({ "min_codons": 100 }),
            context(),
            Some(&snap),
        )
        .unwrap();
    assert_eq!(result.ops.len(), 1, "{:?}", result.message);
    let Op::FeatureAdd { feature, .. } = &result.ops[0] else {
        panic!("not a feature.add")
    };
    let seg = &feature.location.segments[0];
    assert_eq!((seg.start, seg.end), (10, 10 + 3 + 360 + 3));

    // The user inserts 5 residues upstream while the plugin was working.
    session.apply(doc, Op::seq_replace(2, 2, "GGGGG")).unwrap();
    let actor = Actor::Plugin {
        id: p.info.id.clone(),
        version: p.info.version.clone(),
    };
    let report = apply_proposals(&mut session, doc, snap.version, result.ops, &actor).unwrap();
    assert_eq!((report.applied, report.dropped), (1, 0));
    let (_, f) = session.doc(doc).unwrap().features().last().unwrap();
    let seg = &f.location.segments[0];
    assert_eq!((seg.start, seg.end), (15, 15 + 366));
    assert_eq!(
        &session.doc(doc).unwrap().read(seg.start, seg.start + 3),
        b"ATG"
    );
}

#[test]
fn call_overhead() {
    crate::isolated_profile::isolate!();
    let Some(mut p) = plugin() else { return };
    let n = 2000;
    let t = Instant::now();
    for _ in 0..n {
        p.ping().unwrap();
    }
    let ping = t.elapsed() / n;
    eprintln!("startup {:?}, ping {ping:?}", p.startup);
    // The budget is 1 ms; leave room for a loaded machine.
    assert!(ping < Duration::from_millis(1), "ping took {ping:?}");
}
