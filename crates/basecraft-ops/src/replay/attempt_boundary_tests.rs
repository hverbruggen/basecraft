//! Recovery consent is consumed even when the requested document is invalid.
use super::*;

#[test]
fn pre_session_refusal_discards_actual_consent() {
    let Some(root) = crate::isolated_profile::enter(
        "replay::attempt_boundary_tests::pre_session_refusal_discards_actual_consent",
    ) else {
        return;
    };
    let source = root.join("source");
    std::fs::create_dir(&source).unwrap();
    let target = source.join("sequence.fasta");
    std::fs::write(&target, b">a\nACGT\n").unwrap();
    let mut session = Session::recoverable(&source).unwrap();
    let id = session.open(&target).unwrap();
    session
        .apply(
            id,
            basecraft_protocol::Op::SeqReplace {
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    session.flush().unwrap();
    let destination = root.join("final");
    let mut attempt =
        prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict).unwrap();
    let check = attempt.check(0).unwrap();
    assert!(
        attempt
            .advance(
                usize::MAX,
                Some(
                    check
                        .clone()
                        .decide(crate::input_evidence::SavePolicy::Cancel, false)
                )
            )
            .is_err()
    );
    assert!(matches!(
        attempt.advance(
            0,
            Some(check.decide(crate::input_evidence::SavePolicy::Cancel, false))
        ),
        Err(Error::SaveDecisionStale(_))
    ));
    assert!(!destination.exists());
    assert_eq!(std::fs::read(target).unwrap(), b">a\nACGT\n");
}
