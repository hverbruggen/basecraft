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

#[test]
fn recovery_starts_at_completed_save() {
    let Some(root) = crate::isolated_profile::enter("session::retention_tests::recovery_starts_at_completed_save") else { return; };
    let source = root.join("source");
    std::fs::create_dir(&source).unwrap();
    let path = source.join("sequence.fa");
    std::fs::write(&path, b">sequence\nACGT\n").unwrap();
    let mut session = Session::new(&source);
    session.provenance_policy = basecraft_platform::settings::ProvenancePolicy::None;
    session.merge = false;
    let doc = session.open(&path).unwrap();
    let replace = |start, residues: &str| Op::SeqReplace { start, end: start + 1, residues: residues.into() };
    session.apply(doc, replace(0, "T")).unwrap();
    session.save(doc, None).unwrap();
    session.apply(doc, replace(1, "A")).unwrap();
    session.flush().unwrap();
    let recovered = crate::replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
    assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">sequence\nTAGT\n");
    assert_eq!(std::fs::read(&path).unwrap(), b">sequence\nTCGT\n");
    assert!(!source.join(".basecraft/recovery-references").exists());
}
