use super::*;

#[test]
fn changed_original_is_rejected_before_editing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sequence.fa");
    std::fs::write(&path, b">sequence\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    std::fs::write(&path, b">sequence\nTTTT\n").unwrap();
    assert!(session.apply(doc, Op::SeqReplace { start: 0, end: 1, residues: "G".into() }).is_err());
    assert!(!session.can_undo(doc));
    assert!(!session.document_record(doc).unwrap().unsaved);
}
