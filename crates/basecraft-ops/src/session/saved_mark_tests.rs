use super::*;

#[test]
fn undo_redo_and_branching_follow_the_saved_revision() {
    let Some(_profile) = crate::isolated_profile::enter(
        "session::saved_mark_tests::undo_redo_and_branching_follow_the_saved_revision",
    ) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sequence.fa");
    std::fs::write(&path, b">sequence\nACGT\n").unwrap();
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&path).unwrap();
    let edit = |base: &str| Op::SeqReplace {
        start: 0,
        end: 1,
        residues: base.into(),
        record: None,
    };
    assert!(!session.document_record(doc).unwrap().unsaved);
    session.apply(doc, edit("T")).unwrap();
    assert!(session.document_record(doc).unwrap().unsaved);
    session.undo(doc).unwrap();
    assert!(!session.document_record(doc).unwrap().unsaved);
    session.redo(doc).unwrap();
    session.save(doc, None).unwrap();
    assert!(!session.document_record(doc).unwrap().unsaved);
    session.undo(doc).unwrap();
    assert!(session.document_record(doc).unwrap().unsaved);
    session.redo(doc).unwrap();
    assert!(!session.document_record(doc).unwrap().unsaved);
    session.undo(doc).unwrap();
    session.apply(doc, edit("G")).unwrap();
    assert!(session.document_record(doc).unwrap().unsaved);
}
