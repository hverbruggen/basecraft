//! Exercise the real save path with filesystem failures, not call-order observers.
use super::*;
use std::fs;

fn fixture(root: &Path) -> (Session, DocId, PathBuf) {
    let target = root.join("source.fasta");
    fs::write(&target, b">source\nACGT\n").unwrap();
    let mut session = Session::recoverable(root).unwrap();
    let id = session.open(&target).unwrap();
    session
        .apply(
            id,
            Op::SeqReplace {
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    session.flush().unwrap();
    (session, id, target)
}

#[test]
fn failed_intent_keeps_original_and_edits_for_retry() {
    let Some(root) = crate::isolated_profile::enter(
        "session::save_failure_tests::failed_intent_keeps_original_and_edits_for_retry",
    ) else {
        return;
    };
    let (mut session, id, target) = fixture(&root);
    let pending = session.recovery_root().join("pending-save.json");
    fs::create_dir(&pending).unwrap();
    assert!(session.save(id, None).is_err());
    assert_eq!(fs::read(&target).unwrap(), b">source\nACGT\n");
    assert!(session.can_undo(id));
    fs::remove_dir(pending).unwrap();
    session.save(id, None).unwrap();
    assert!(fs::read_to_string(target).unwrap().contains("TCGT"));
}

#[test]
fn failed_provenance_keeps_recovery_and_retry_accepts_written_data() {
    let Some(root) = crate::isolated_profile::enter(
        "session::save_failure_tests::failed_provenance_keeps_recovery_and_retry_accepts_written_data",
    ) else {
        return;
    };
    let (mut session, id, target) = fixture(&root);
    let bundle = crate::provenance::companion(&target);
    fs::write(&bundle, b"blocked").unwrap();
    assert!(session.save(id, None).is_err());
    assert!(fs::read_to_string(&target).unwrap().contains("TCGT"));
    assert!(session.recovery_root().join("pending-save.json").is_file());
    assert!(session.can_undo(id));
    fs::remove_file(bundle).unwrap();
    session.save(id, None).unwrap();
    assert!(!session.recovery_root().join("pending-save.json").exists());
    assert!(!session.document_record(id).unwrap().unsaved);
}

#[test]
fn new_destination_and_history_preserve_existing_bytes() {
    let Some(root) = crate::isolated_profile::enter(
        "session::save_failure_tests::new_destination_and_history_preserve_existing_bytes",
    ) else {
        return;
    };
    let (mut session, id, target) = fixture(&root);
    assert!(session.save_history(id).is_err());
    let other = root.join("other.fasta");
    fs::write(&other, b"unrelated bytes").unwrap();
    assert!(session.save_new(id, Some(&other)).is_err());
    assert_eq!(fs::read(other).unwrap(), b"unrelated bytes");
    session.save(id, None).unwrap();
    let before = crate::input_evidence::stamp(&target).unwrap();
    session.save_history(id).unwrap();
    assert_eq!(crate::input_evidence::stamp(&target).unwrap(), before);
}
