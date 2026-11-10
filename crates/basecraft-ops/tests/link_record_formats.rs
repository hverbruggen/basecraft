//! Byte oracles exercised against original writers before the protocol move.
use basecraft_ops::{
    files, input_evidence,
    links::{self, AlignmentLink},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

// Frozen original wire declaration. This is a test oracle, never a production record.
#[derive(Serialize)]
struct OriginalLink {
    schema: u32,
    source: PathBuf,
    source_hash: String,
    snapshot: PathBuf,
    snapshot_hash: String,
    alignment_version: u64,
    inferred: bool,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

fn capture(name: &str, metadata: &Path, expected: &OriginalLink, newline: bool) {
    let actual = std::fs::read(metadata).unwrap();
    let mut original = serde_json::to_vec_pretty(expected).unwrap();
    if newline {
        original.push(b'\n');
    }
    assert_eq!(actual, original, "{name}");
    if let Some(root) = std::env::var_os("BASECRAFT_RECORD_CAPTURE") {
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(Path::new(&root).join(format!("{name}.json")), actual).unwrap();
        std::fs::write(
            Path::new(&root).join(format!("{name}.original.json")),
            original,
        )
        .unwrap();
    }
}

#[test]
fn original_snapshot_and_view_writers() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("ünicode source.afa");
    let folder = root.path().join("trees");
    std::fs::create_dir(&folder).unwrap();
    let tree = folder.join("origin.nwk");
    std::fs::write(&source, b">a\nAC\n>b\nAT\n").unwrap();
    std::fs::write(&tree, "(a,b);").unwrap();
    let hash = basecraft_io::hash_file(&source).unwrap();
    let outputs = AlignmentLink::create(&tree, &source, &source, 7, true, None).unwrap();
    let mut expected = OriginalLink {
        schema: 1,
        source: "../ünicode source.afa".into(),
        source_hash: hash.clone(),
        snapshot: outputs[0].file_name().unwrap().into(),
        snapshot_hash: hash,
        alignment_version: 7,
        inferred: true,
        extra: BTreeMap::new(),
    };
    capture(
        "link-snapshot-inferred",
        &links::sidecar(&tree),
        &expected,
        false,
    );
    let doc = files::load_tree(&tree).unwrap().doc;
    let outputs = AlignmentLink::associate(&tree, &doc, &source).unwrap();
    expected.snapshot = outputs[0].file_name().unwrap().into();
    expected.alignment_version = 0;
    expected.inferred = false;
    capture(
        "link-explicit-view",
        &links::view_sidecar(&tree),
        &expected,
        false,
    );
    // Equal labels do not turn an explicit presentation association into inference.
    assert!(AlignmentLink::read(&tree).unwrap().inferred);
    assert!(!AlignmentLink::read_view(&tree).unwrap().inferred);
    let mut link = AlignmentLink::read_view(&tree).unwrap();
    link.extra.insert(
        "future".into(),
        serde_json::json!({"z":[null,{"b":2,"a":1}],"a":true}),
    );
    let shared = folder.join("shared.nwk");
    let metadata = link.share_to(&tree, &shared).unwrap();
    expected.extra = link.extra.clone();
    capture("link-snapshot-extra", &metadata, &expected, false);
}

#[test]
fn original_referenced_and_shared_writers() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    let tree = root.path().join("ü tree.nwk");
    std::fs::write(&source, b">a\nAC\n>b\nAT\n").unwrap();
    std::fs::write(&tree, "(a,b);").unwrap();
    let hash = basecraft_io::hash_file(&source).unwrap();
    let run = input_evidence::reference_input_with_dependencies(
        &tree,
        &source,
        &hash,
        &[],
        "link fixture",
    )
    .unwrap();
    AlignmentLink::create_referenced(&tree, &source, &source, 9, &run).unwrap();
    let mut expected = OriginalLink {
        schema: 2,
        source: "source.afa".into(),
        source_hash: hash.clone(),
        snapshot: PathBuf::new(),
        snapshot_hash: hash,
        alignment_version: 9,
        inferred: true,
        extra: [(
            "analysis_run".into(),
            serde_json::json!(run.directory().strip_prefix(root.path()).unwrap()),
        )]
        .into(),
    };
    capture("link-referenced", &links::sidecar(&tree), &expected, true);
    let mut link = AlignmentLink::read(&tree).unwrap();
    link.extra
        .insert("future".into(), serde_json::json!({"nested":[null,"kept"]}));
    let shared = root.path().join("shared.nwk");
    std::fs::write(&shared, "(a,b);").unwrap();
    let metadata = link.share_to(&tree, &shared).unwrap();
    expected.extra = link.extra.clone();
    capture("link-referenced-extra", &metadata, &expected, true);
    assert!(
        AlignmentLink::read(&shared)
            .unwrap()
            .checked_snapshot(&shared, &files::load_tree(&shared).unwrap().doc)
            .is_ok()
    );
}
