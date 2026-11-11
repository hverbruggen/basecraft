//! Public link behavior; the protocol registry checks the serialized goldens.
use basecraft_ops::{
    files, input_evidence,
    links::{self, AlignmentLink, AlignmentLinkExt},
};

#[test]
fn snapshot_and_view_associations_remain_distinct() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    let tree = root.path().join("tree.nwk");
    std::fs::write(&source, b">a\nAC\n>b\nAT\n").unwrap();
    std::fs::write(&tree, "(a,b);").unwrap();
    let outputs = AlignmentLink::create(&tree, &source, &source, 7, true, None).unwrap();
    assert_eq!(
        std::fs::read(&outputs[0]).unwrap(),
        std::fs::read(&source).unwrap()
    );
    let doc = files::load_tree(&tree).unwrap().doc;
    AlignmentLink::associate(&tree, &doc, &source).unwrap();
    assert!(AlignmentLink::read(&tree).unwrap().inferred);
    assert!(!AlignmentLink::read_view(&tree).unwrap().inferred);
}

#[test]
fn shared_reference_verifies_the_same_input() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    let tree = root.path().join("tree.nwk");
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
    let shared = root.path().join("shared.nwk");
    std::fs::write(&shared, "(a,b);").unwrap();
    AlignmentLink::read(&tree)
        .unwrap()
        .share_to(&tree, &shared)
        .unwrap();
    assert!(
        AlignmentLink::read(&shared)
            .unwrap()
            .checked_snapshot(&shared, &files::load_tree(&shared).unwrap().doc)
            .is_ok()
    );
}

#[test]
fn reader_keeps_extensions_and_existing_version_and_missing_field_checks() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree.nwk");
    std::fs::write(&tree, "(a,b);").unwrap();
    let metadata = links::sidecar(&tree);
    let bytes = include_bytes!("../../../testdata/fixtures/records-v1/link-snapshot-extra.json");
    let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    // Reader retains extras and tolerates missing source; evidence use checks it later.
    std::fs::write(&metadata, serde_json::to_vec(&value).unwrap()).unwrap();
    let link = AlignmentLink::read(&tree).unwrap();
    assert_eq!(link.extra["future"], value["future"]);
    for version in [0, 3, u32::MAX] {
        value["schema"] = version.into();
        std::fs::write(&metadata, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            AlignmentLink::read(&tree)
                .unwrap_err()
                .to_string()
                .contains("unsupported tree–alignment link version")
        );
    }
    value["schema"] = 1.into();
    value.as_object_mut().unwrap().remove("inferred");
    assert!(serde_json::from_value::<AlignmentLink>(value.clone()).is_err());
    std::fs::write(&metadata, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(AlignmentLink::read(&tree).is_err());
    std::fs::write(&metadata, b"broken JSON").unwrap();
    assert!(AlignmentLink::read(&tree).is_err());
}
