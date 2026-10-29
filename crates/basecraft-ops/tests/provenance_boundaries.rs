use basecraft_ops::{Session, files};
use basecraft_protocol::Op;

#[test]
fn same_path_replacement_cannot_reuse_tree_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "((a,b)90,c);\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    session
        .apply(
            doc,
            Op::TreeAddSupport {
                name: "Imported".into(),
                source: "source".into(),
                values: vec![None, Some("42".into())],
                kind: Default::default(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    std::fs::write(&path, "((c,b)10,a);\n").unwrap();
    assert!(
        files::load_tree(&path).is_err(),
        "stale metadata was accepted"
    );
}

#[test]
fn undone_import_is_absent_from_methods() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("tree.nwk");
    let source = dir.path().join("source.nwk");
    std::fs::write(&target, "((a,b)90,c);\n").unwrap();
    std::fs::write(&source, "((a,b)42,c);\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&target).unwrap();
    session.import_support(doc, &source, "Imported").unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(
        report.methods.contains("import_support"),
        "retained import absent from Methods"
    );
    session.undo(doc).unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(!report.methods.contains("import_support"));
}
