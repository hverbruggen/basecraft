use super::*;
use crate::session::save_attempt_observation::{
    self as save, Call, Observer,
    tests::{assert_writer, fixture, inventory, observer},
};

#[derive(Default)]
struct ObserverAdvance {
    save: Option<Observer>,
    facts: Option<save::Facts>,
    session_return: Call,
    active: Option<AdvanceSite>,
    diagnostics: Vec<(AdvanceSite, Error)>,
    artifacts_fault: Option<AdvanceSite>,
}
impl AdvanceHooks for ObserverAdvance {
    fn at(&mut self, site: AdvanceSite) {
        self.active = Some(site);
    }
    fn secondary(&mut self, site: AdvanceSite, error: Error) {
        self.diagnostics.push((site, error));
    }
    fn discard(&mut self, primary: Error, discarded: Result<Option<()>>) -> Error {
        if let Err(error) = discarded {
            self.secondary(AdvanceSite::Discard, error);
        }
        primary
    }
    fn artifacts(&mut self, path: &Path, site: AdvanceSite) -> Result<Vec<ArtifactObservation>> {
        if self.artifacts_fault == Some(site) {
            self.artifacts_fault = None;
            return Err(std::io::Error::other("injected artifact inspection").into());
        }
        artifacts(path)
    }
    fn save(
        &mut self,
        session: &mut Session,
        id: DocId,
        path: Option<&Path>,
        decision: Option<crate::session::SaveDecision>,
    ) -> Result<crate::session::SaveOutcome> {
        self.session_return = Call::Entered;
        let attempt = session.save_recovery_observed(
            id,
            path,
            decision,
            self.save.take().unwrap_or_default(),
        );
        if attempt.result.is_ok() {
            self.session_return = Call::ReturnedOk;
        }
        self.facts = Some(attempt.facts);
        attempt.result
    }
}
// Returned data contains no Session, guard, callbacks or TempDir.
struct ObservedAdvance {
    result: Result<()>,
    facts: Option<save::Facts>,
    session_return: Call,
    failure: Option<AdvanceSite>,
    diagnostics: Vec<(AdvanceSite, Error)>,
}
impl RecoveryAttempt {
    fn advance_observed(
        &mut self,
        document: usize,
        decision: Option<crate::session::SaveDecision>,
        mut hooks: ObserverAdvance,
    ) -> ObservedAdvance {
        let result = self.advance_with(document, decision, &mut hooks);
        let failure = result.is_err().then_some(hooks.active).flatten();
        ObservedAdvance {
            result,
            facts: hooks.facts,
            session_return: hooks.session_return,
            failure,
            diagnostics: hooks.diagnostics,
        }
    }
}
#[test]
fn actual_recovery_four_states() {
    let Some(root) = crate::isolated_profile::enter(
        "replay::attempt_boundary_tests::actual_recovery_four_states",
    ) else {
        return;
    };
    // Recovery owns an unlogged, untracked Session. Journal, provenance and
    // pending-remove failures are covered by the actual recoverable Session
    // tests; manufacturing those owners here would change the workflow.
    for case in [0, 1, 4, 5, 7] {
        let root = root.join(case.to_string());
        std::fs::create_dir_all(&root).unwrap();
        let (source, _, _) = fixture(&root);
        let before_source = inventory(&root.join("source"));
        let mut attempt = prepare_recovery(
            source.recovery_root(),
            &root.join("final"),
            RecoveryMode::Strict,
        )
        .unwrap();
        let check = attempt.check(0).unwrap();
        let observed = attempt.advance_observed(
            0,
            Some(check.decide(crate::input_evidence::SavePolicy::Cancel, false)),
            ObserverAdvance {
                save: Some(observer(case)),
                ..Default::default()
            },
        );
        match (case, observed.result.as_ref().unwrap_err()) {
            (0, Error::Format(basecraft_io::Error::Io(error))) => {
                assert!(error.raw_os_error().is_some())
            }
            (1 | 4 | 5 | 7, Error::Io(error)) => {
                assert_eq!(error.to_string(), "injected boundary failure")
            }
            (_, error) => panic!("unexpected recovery primary: {error:?}"),
        }
        assert_writer(observed.facts.as_ref().unwrap(), case);
        let facts = observed.facts.as_ref().unwrap();
        if case >= 4 {
            assert_eq!(facts.journal_present, Some(false));
            assert_eq!(facts.provenance, Call::NotEntered);
            assert!(!facts.pending_removed);
        }
        assert_eq!(
            observed.session_return,
            if case == 7 {
                Call::ReturnedOk
            } else {
                Call::Entered
            }
        );
        assert_eq!(attempt.documents[0].written, case == 7);
        assert!(attempt.documents[0].saved.is_none());
        assert_eq!(
            observed.failure,
            Some(if case == 7 {
                AdvanceSite::Observation
            } else {
                AdvanceSite::Session
            })
        );
        assert_eq!(inventory(&root.join("source")), before_source);
        let before_drop = inventory(&root);
        drop(observed);
        assert_eq!(inventory(&root), before_drop);
        if case == 7 {
            let check = attempt.check(0).unwrap();
            let finalizing = attempt.advance_observed(
                0,
                Some(check.decide(crate::input_evidence::SavePolicy::Cancel, false)),
                ObserverAdvance::default(),
            );
            assert!(finalizing.result.is_ok());
            assert_eq!(finalizing.session_return, Call::NotEntered);
            assert!(finalizing.facts.is_none());
        }
    }
}
#[test]
fn distinct_artifact_and_precomputed_observation_errors() {
    let Some(root) = crate::isolated_profile::enter(
        "replay::attempt_boundary_tests::distinct_artifact_and_precomputed_observation_errors",
    ) else {
        return;
    };
    for case in [0, 7] {
        let root = root.join(case.to_string());
        std::fs::create_dir_all(&root).unwrap();
        let (source, _, _) = fixture(&root);
        let mut attempt = prepare_recovery(
            source.recovery_root(),
            &root.join("final"),
            RecoveryMode::Strict,
        )
        .unwrap();
        let check = attempt.check(0).unwrap();
        let observed = attempt.advance_observed(
            0,
            Some(check.decide(crate::input_evidence::SavePolicy::Cancel, false)),
            ObserverAdvance {
                save: Some(observer(case)),
                artifacts_fault: Some(if case == 0 {
                    AdvanceSite::FailedArtifacts
                } else {
                    AdvanceSite::FirstArtifacts
                }),
                ..Default::default()
            },
        );
        assert!(observed.result.is_err());
        assert_eq!(observed.diagnostics.len(), 1);
        if case == 7 {
            assert_eq!(
                observed.result.as_ref().unwrap_err().to_string(),
                "injected artifact inspection"
            );
            assert_eq!(
                observed.diagnostics[0].1.to_string(),
                "injected boundary failure"
            );
        }
        assert_eq!(
            observed.diagnostics[0].0,
            if case == 0 {
                AdvanceSite::FailedArtifacts
            } else {
                AdvanceSite::Observation
            }
        );
        assert_eq!(attempt.documents[0].written, case == 7);
    }
}

#[test]
fn same_bytes_new_identity_reaches_recovery() {
    let Some(root) = crate::isolated_profile::enter(
        "replay::attempt_boundary_tests::same_bytes_new_identity_reaches_recovery",
    ) else {
        return;
    };
    let (source, _, _) = fixture(&root);
    let mut attempt = prepare_recovery(
        source.recovery_root(),
        &root.join("final"),
        RecoveryMode::Strict,
    )
    .unwrap();
    let check = attempt.check(0).unwrap();
    let observed = attempt.advance_observed(
        0,
        Some(check.decide(crate::input_evidence::SavePolicy::Cancel, false)),
        ObserverAdvance {
            save: Some(save::tests::replacing_observer()),
            ..Default::default()
        },
    );
    assert!(observed.result.is_ok());
    assert_eq!(observed.session_return, Call::ReturnedOk);
    save::tests::assert_distinct_identity(observed.facts.as_ref().unwrap());
    let before = inventory(&root);
    drop(observed);
    assert_eq!(inventory(&root), before);
    assert!(attempt.documents[0].written);
}

#[test]
fn pre_session_refusal_discards_actual_consent() {
    let Some(root) = crate::isolated_profile::enter(
        "replay::attempt_boundary_tests::pre_session_refusal_discards_actual_consent",
    ) else {
        return;
    };
    let (source, _, _) = fixture(&root);
    let mut attempt = prepare_recovery(
        source.recovery_root(),
        &root.join("final"),
        RecoveryMode::Strict,
    )
    .unwrap();
    let check = attempt.check(0).unwrap();
    let before = inventory(&root);
    let observed = attempt.advance_observed(
        usize::MAX,
        Some(
            check
                .clone()
                .decide(crate::input_evidence::SavePolicy::Cancel, false),
        ),
        ObserverAdvance::default(),
    );
    assert!(observed.result.is_err());
    assert_eq!(observed.failure, Some(AdvanceSite::Validate));
    assert_eq!(observed.session_return, Call::NotEntered);
    assert!(observed.facts.is_none());
    assert!(observed.diagnostics.is_empty());
    assert_eq!(inventory(&root), before);
    let spent = attempt.advance_observed(
        0,
        Some(check.decide(crate::input_evidence::SavePolicy::Cancel, false)),
        ObserverAdvance::default(),
    );
    assert!(matches!(spent.result, Err(Error::SaveDecisionStale(_))));
    assert_eq!(spent.facts.as_ref().unwrap().document, Call::NotEntered);
    assert!(!attempt.documents[0].written);
}
