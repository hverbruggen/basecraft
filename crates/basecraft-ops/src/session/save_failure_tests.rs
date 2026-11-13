//! Exercise the real save path with filesystem failures, not call-order observers.
use super::*;
use std::fs;

fn fixture(root: &Path) -> (Session, DocId, PathBuf) {
    let target = root.join("source.fasta");
    fs::write(&target, b">source\nACGT\n").unwrap();
    let mut session = Session::recoverable(root).unwrap();
    let id = session.open(&target).unwrap();
    session.apply(id, Op::seq_replace(0, 1, "T")).unwrap();
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
    assert!(session.can_undo(id).unwrap());
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
    let bundle = crate::records::provenance::companion(&target);
    fs::write(&bundle, b"blocked").unwrap();
    assert!(session.save(id, None).is_err());
    assert!(fs::read_to_string(&target).unwrap().contains("TCGT"));
    assert!(session.recovery_root().join("pending-save.json").is_file());
    assert!(session.can_undo(id).unwrap());
    fs::remove_file(bundle).unwrap();
    session.save(id, None).unwrap();
    assert!(!session.recovery_root().join("pending-save.json").exists());
    assert!(!session.document_record(id).unwrap().unsaved);
}

#[test]
fn crash_before_and_after_replacement_recovers_the_edits() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    for after in [false, true] {
        let folder = root.path().join(if after { "after" } else { "before" });
        fs::create_dir(&folder).unwrap();
        let (mut session, id, target) = fixture(&folder);
        let recovery = session.recovery_root().to_path_buf();
        let obstruction = if after {
            crate::records::provenance::companion(&target)
        } else {
            recovery.join("pending-save.json")
        };
        if after {
            fs::write(&obstruction, b"blocked").unwrap();
        } else {
            fs::create_dir(&obstruction).unwrap();
        }
        assert!(session.save(id, None).is_err());
        assert_eq!(
            fs::read(&target).unwrap(),
            if after {
                b">source\nTCGT\n"
            } else {
                b">source\nACGT\n"
            }
        );
        if after {
            fs::remove_file(obstruction).unwrap();
        } else {
            fs::remove_dir(obstruction).unwrap();
        }
        drop(session);
        let restored =
            crate::records::replay::recover(&recovery, &folder.join("restored")).unwrap();
        assert_eq!(fs::read(&restored[0]).unwrap(), b">source\nTCGT\n");
    }
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
    let before = crate::records::input_evidence::stamp(&target).unwrap();
    session.save_history(id).unwrap();
    assert_eq!(
        crate::records::input_evidence::stamp(&target).unwrap(),
        before
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WriteStep {
    Replace,
    Journal,
    RemoveIntent,
    RecoveryCheck,
}
thread_local! { static FAILURE: std::cell::Cell<Option<WriteStep>> = const { std::cell::Cell::new(None) }; }

/// Make the next save on this thread fail at `step`.
pub(super) fn inject(step: WriteStep) {
    FAILURE.with(|failure| failure.set(Some(step)));
}

// One test-only failure seam, with no checks, callbacks or alternate writer.
pub(super) fn fail_write(step: WriteStep) -> Result<()> {
    if FAILURE.with(|failure| failure.get() == Some(step)) {
        FAILURE.with(|failure| failure.set(None));
        return Err(std::io::Error::other("injected save write failure").into());
    }
    Ok(())
}

#[test]
fn late_write_failures_retain_recovery_and_retry() {
    let Some(root) = crate::isolated_profile::enter(
        "session::save_failure_tests::late_write_failures_retain_recovery_and_retry",
    ) else {
        return;
    };
    for (name, step) in [
        ("journal", WriteStep::Journal),
        ("cleanup", WriteStep::RemoveIntent),
    ] {
        let root = root.join(name);
        fs::create_dir(&root).unwrap();
        let (mut session, id, target) = fixture(&root);
        FAILURE.with(|failure| failure.set(Some(step)));
        assert!(session.save(id, None).is_err());
        assert!(fs::read_to_string(&target).unwrap().contains("TCGT"));
        assert!(session.recovery_root().join("pending-save.json").is_file());
        assert!(session.can_undo(id).unwrap());
        session.save(id, None).unwrap();
        assert!(!session.recovery_root().join("pending-save.json").exists());
        assert!(fs::read_to_string(target).unwrap().contains("TCGT"));
    }
}

#[test]
fn failed_save_journal_keeps_previous_history_and_recovers_written_data() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let (mut session, id, target) = fixture(root.path());
    session.save(id, None).unwrap();
    let bundle = crate::records::provenance::companion(&target);
    let manifest = fs::read(bundle.join("manifest.json")).unwrap();
    let records = fs::read_dir(bundle.join("records")).unwrap().count();
    session.apply(id, Op::seq_replace(1, 2, "A")).unwrap();
    FAILURE.with(|failure| failure.set(Some(WriteStep::Journal)));
    assert!(session.save(id, None).is_err());
    assert_eq!(fs::read(bundle.join("manifest.json")).unwrap(), manifest);
    assert_eq!(
        fs::read_dir(bundle.join("records")).unwrap().count(),
        records
    );
    let recovery = session.recovery_root().to_path_buf();
    assert!(recovery.join("pending-save.json").is_file());
    assert_eq!(fs::read(&target).unwrap(), b">source\nTAGT\n");
    drop(session);
    let restored =
        crate::records::replay::recover(&recovery, &root.path().join("restored")).unwrap();
    assert_eq!(fs::read(&restored[0]).unwrap(), b">source\nTAGT\n");
}

#[test]
fn recovery_inspection_failure_retries_without_replacing_written_data() {
    let Some(root) = crate::isolated_profile::enter(
        "session::save_failure_tests::recovery_inspection_failure_retries_without_replacing_written_data",
    ) else {
        return;
    };
    let (session, _, original) = fixture(&root);
    let mut recovery = crate::records::replay::prepare_recovery(
        session.recovery_root(),
        &root.join("recovered"),
        crate::records::replay::RecoveryMode::Strict,
    )
    .unwrap();
    let path = recovery.check(0).unwrap().target().to_owned();
    FAILURE.with(|failure| failure.set(Some(WriteStep::RecoveryCheck)));
    assert!(recovery.advance(0, None).is_err());
    let written = crate::records::input_evidence::stamp(&path).unwrap();
    let check = recovery.check(0).unwrap();
    recovery
        .advance(
            0,
            Some(check.decide(crate::records::input_evidence::SavePolicy::Cancel, false)),
        )
        .unwrap();
    assert_eq!(
        crate::records::input_evidence::stamp(&path).unwrap(),
        written
    );
    let completed = recovery.publish().unwrap();
    assert_eq!(fs::read(&original).unwrap(), b">source\nACGT\n");
    assert!(
        fs::read_to_string(&completed.outputs[0])
            .unwrap()
            .contains("TCGT")
    );
}
