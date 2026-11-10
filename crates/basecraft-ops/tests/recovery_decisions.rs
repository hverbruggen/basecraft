//! Retained recovery authority and publication, using isolated real files.
use basecraft_ops::{
    Error, Session,
    input_evidence::SavePolicy,
    replay::{self, RecoveryMode},
};
use basecraft_protocol::Op;
use std::path::{Path, PathBuf};
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

fn source(root: &Path, count: usize) -> Session {
    let directory = root.join("source");
    std::fs::create_dir_all(&directory).unwrap();
    let mut session = Session::recoverable(&directory).unwrap();
    for index in 0..count {
        let file = directory.join(format!("{index}.fasta"));
        std::fs::write(&file, b">gene\nACGT\n").unwrap();
        let doc = session.open(&file).unwrap();
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 1,
                    end: 2,
                    residues: "T".into(),
                },
            )
            .unwrap();
    }
    session.flush().unwrap();
    session
}
fn registry() -> PathBuf {
    basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-stores.json")
}
fn damage() {
    let path = registry();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"damaged inventory").unwrap();
}
fn required(error: Error) -> basecraft_ops::session::SaveCheck {
    match error {
        Error::SaveCheckRequired(c) | Error::SaveDecisionStale(c) => *c,
        other => panic!("{other:?}"),
    }
}

#[test]
fn complete_multiple_documents_publish_once_without_rewriting() {
    let Some(root) =
        isolated_profile::enter("complete_multiple_documents_publish_once_without_rewriting")
    else {
        return;
    };
    let session = source(&root, 2);
    let destination = root.join("recovered");
    let mut attempt =
        replay::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let check = attempt.check(0).unwrap();
    let stage = check.target().parent().unwrap().to_path_buf();
    attempt.advance(0, None).unwrap();
    let first = basecraft_platform::files::identity(check.target()).unwrap();
    attempt.advance(1, None).unwrap();
    let complete = attempt.publish().unwrap();
    assert_eq!(complete.outputs.len(), 2);
    assert_eq!(
        basecraft_platform::files::identity(&complete.outputs[0]).unwrap(),
        first
    );
    assert!(!stage.exists());
    for output in complete.outputs {
        assert_eq!(std::fs::read(output).unwrap(), b">gene\nATGT\n");
    }
    assert!(attempt.publish().is_err());
}

#[test]
fn changed_inventory_requires_fresh_final_consent_without_resaving() {
    let Some(root) =
        isolated_profile::enter("changed_inventory_requires_fresh_final_consent_without_resaving")
    else {
        return;
    };
    let session = source(&root, 2);
    let destination = root.join("recovered");
    let mut attempt =
        replay::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let path = attempt.check(0).unwrap().target().to_path_buf();
    attempt.advance(0, None).unwrap();
    let first = basecraft_platform::files::identity(&path).unwrap();
    damage();
    let second = required(attempt.advance(1, None).unwrap_err());
    attempt
        .advance(1, Some(second.decide(SavePolicy::Cancel, true)))
        .unwrap();
    let fresh = required(attempt.publish().unwrap_err());
    assert!(!destination.exists());
    let refused = required(
        attempt
            .advance(0, Some(fresh.decide(SavePolicy::Cancel, false)))
            .unwrap_err(),
    );
    attempt
        .advance(0, Some(refused.decide(SavePolicy::Cancel, true)))
        .unwrap();
    assert_eq!(basecraft_platform::files::identity(&path).unwrap(), first);
    attempt.publish().unwrap();
}

#[test]
fn receipts_are_single_use_session_document_and_phase_bound() {
    let Some(root) =
        isolated_profile::enter("receipts_are_single_use_session_document_and_phase_bound")
    else {
        return;
    };
    let session = source(&root, 2);
    damage();
    let mut a = replay::prepare_recovery(
        session.recovery_root(),
        &root.join("a"),
        RecoveryMode::Strict,
    )
    .unwrap();
    let mut b = replay::prepare_recovery(
        session.recovery_root(),
        &root.join("b"),
        RecoveryMode::Strict,
    )
    .unwrap();
    let old = a.check(0).unwrap();
    assert!(matches!(
        b.advance(0, Some(old.clone().decide(SavePolicy::Cancel, true))),
        Err(Error::SaveDecisionStale(_))
    ));
    // A foreign attempt cannot spend the originating receipt.
    a.advance(0, Some(old.clone().decide(SavePolicy::Cancel, true)))
        .unwrap();
    assert!(matches!(
        a.advance(0, Some(old.decide(SavePolicy::Cancel, true))),
        Err(Error::SaveDecisionStale(_))
    ));
    let first = a.check(1).unwrap();
    let newest = a.check(1).unwrap();
    assert!(matches!(
        a.advance(1, Some(first.decide(SavePolicy::Cancel, true))),
        Err(Error::SaveDecisionStale(_))
    ));
    // The stale attempt publishes a refreshed check, superseding newest too.
    assert!(matches!(
        a.advance(1, Some(newest.decide(SavePolicy::Cancel, true))),
        Err(Error::SaveDecisionStale(_))
    ));
    let wrong = a.check(1).unwrap();
    assert!(matches!(
        a.advance(0, Some(wrong.clone().decide(SavePolicy::Cancel, true))),
        Err(Error::SaveDecisionStale(_))
    ));
    assert!(matches!(
        a.advance(1, Some(wrong.decide(SavePolicy::Cancel, true))),
        Err(Error::SaveDecisionStale(_))
    ));
}

#[test]
fn same_bytes_replacement_and_cancel_preserve_originals() {
    let Some(root) =
        isolated_profile::enter("same_bytes_replacement_and_cancel_preserve_originals")
    else {
        return;
    };
    let session = source(&root, 1);
    let mut attempt = replay::prepare_recovery(
        session.recovery_root(),
        &root.join("out"),
        RecoveryMode::Strict,
    )
    .unwrap();
    let target = attempt.check(0).unwrap().target().to_path_buf();
    let stage = target.parent().unwrap().to_path_buf();
    attempt.advance(0, None).unwrap();
    let bytes = std::fs::read(&target).unwrap();
    basecraft_io::write_atomic(&target, |w| w.write_all(&bytes)).unwrap();
    assert!(attempt.publish().is_err());
    attempt.cancel();
    assert!(!stage.exists());
    assert!(!root.join("out").exists());
    assert_eq!(
        std::fs::read(root.join("source/0.fasta")).unwrap(),
        b">gene\nACGT\n"
    );
    assert!(
        !basecraft_ops::log::read_all(session.recovery_root())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn destination_competitors_and_replaced_parent_fail_closed() {
    let Some(root) =
        isolated_profile::enter("destination_competitors_and_replaced_parent_fail_closed")
    else {
        return;
    };
    let session = source(&root, 1);
    for kind in 0..4 {
        let destination = root.join(format!("competitor-{kind}"));
        let mut attempt =
            replay::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
                .unwrap();
        attempt.advance(0, None).unwrap();
        match kind {
            0 => std::fs::write(&destination, b"competitor").unwrap(),
            1 => std::fs::create_dir(&destination).unwrap(),
            2 => {
                std::fs::create_dir(&destination).unwrap();
                std::fs::write(destination.join("owned"), b"competitor").unwrap();
            }
            _ => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(root.join("missing"), &destination).unwrap();
            }
        }
        assert!(attempt.publish().is_err());
        assert!(std::fs::symlink_metadata(&destination).is_ok());
        assert!(
            replay::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
                .is_err()
        );
    }
    let parent = root.join("parent");
    std::fs::create_dir(&parent).unwrap();
    let mut attempt = replay::prepare_recovery(
        session.recovery_root(),
        &parent.join("out"),
        RecoveryMode::Strict,
    )
    .unwrap();
    attempt.advance(0, None).unwrap();
    std::fs::rename(&parent, root.join("old-parent")).unwrap();
    std::fs::create_dir(&parent).unwrap();
    assert!(attempt.publish().is_err());
    assert!(!parent.join("out").exists());
}
