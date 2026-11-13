//! One-attempt incomplete-evidence decisions at the authoritative save boundary.
use basecraft_ops::{
    DocId, Error, Session,
    records::ArtifactRun,
    records::{self, SavePolicy},
    session::{SaveCheck, SaveWarning},
};
use basecraft_platform::paths::AppPaths;
use basecraft_protocol::Op;
use std::{path::Path, process::Command};

const ORIGINAL: &[u8] = b">gene\nACGT\n";

// Reuse the budget/kind-conformance child-process pattern. Command::env selects
// the profile before any AppPaths/Session initialization; no process-global env
// mutation or serial lock is needed in the parallel parent harness.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

fn isolated(name: &str, test: impl FnOnce(&Path)) {
    if let Some(root) = isolated_profile::enter(name) {
        test(&root);
    }
}

fn folder(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn reference(root: &Path, source: &Path) -> ArtifactRun {
    records::reference_input_with_dependencies(
        &root.join("result.txt"),
        source,
        &basecraft_io::hash_file(source).unwrap(),
        &[],
        "save protection regression",
        None,
    )
    .unwrap()
}

fn edited(root: &Path, file: &Path) -> (Session, DocId) {
    let mut session = Session::recoverable(root).unwrap();
    let doc = session.open(file).unwrap();
    session
        .apply(
            doc,
            Op::SeqReplace {
                start: 1,
                end: 2,
                residues: "T".into(),
                record: None,
            },
        )
        .unwrap();
    (session, doc)
}

fn damage(root: &Path, source: &Path) -> ArtifactRun {
    let unrelated = root.join("unrelated.fasta");
    std::fs::write(&unrelated, ORIGINAL).unwrap();
    let run = reference(&folder(root, "distant"), &unrelated);
    assert_ne!(unrelated, source);
    let original = std::fs::read(run.artifact("input.json").unwrap()).unwrap();
    std::fs::write(run.directory().join("test-original-input"), original).unwrap();
    std::fs::write(run.artifact("input.json").unwrap(), b"damaged a").unwrap();
    // The index refresh cannot read the damaged ledger, so the store's coverage
    // becomes unknown for every save until the ledger is repaired.
    records::register(&run).unwrap();
    run
}

fn required(error: Error) -> SaveCheck {
    match error {
        Error::SaveCheckRequired(check) | Error::SaveDecisionStale(check) => *check,
        other => panic!("expected typed save check, got {other:?}"),
    }
}

fn setup(root: &Path) -> (Session, DocId, std::path::PathBuf) {
    let data = folder(root, "data");
    let file = data.join("gene.fasta");
    std::fs::write(&file, ORIGINAL).unwrap();
    let (session, doc) = edited(&data, &file);
    (session, doc, file)
}

fn assert_recovery(session: &mut Session, root: &Path) {
    let before =
        serde_json::to_value(basecraft_ops::log::read_all(session.recovery_root()).unwrap())
            .unwrap();
    // Recovery itself uses conservative Session saving. Do not invent blanket
    // consent for its private staging destination, or claim recovery succeeded
    // while the same global inspection remains incomplete.
    assert!(matches!(
        records::recover(session.recovery_root(), &root.join("recovered")),
        Err(Error::SaveCheckRequired(_))
    ));
    assert!(!root.join("recovered").exists());
    assert_eq!(
        serde_json::to_value(basecraft_ops::log::read_all(session.recovery_root()).unwrap())
            .unwrap(),
        before
    );
    // Repair only the deliberately damaged test ledger, then prove the retained
    // journal reconstructs exactly the original accepted edit, not changed disk bytes.
    let runs = root.join("distant/.basecraft/analyses-v1/runs");
    for entry in std::fs::read_dir(runs).unwrap() {
        let run = entry.unwrap().path();
        let input: records::SubmittedInput =
            serde_json::from_slice(&std::fs::read(run.join("test-original-input")).unwrap())
                .unwrap();
        records::write_json(&run.join("input.json"), &input).unwrap();
    }
    let outputs = records::recover(session.recovery_root(), &root.join("recovered")).unwrap();
    assert_eq!(std::fs::read(&outputs[0]).unwrap(), b">gene\nATGT\n");
}

#[test]
fn incomplete_default_refuses_and_explicit_attempt_returns_warning() {
    isolated(
        "incomplete_default_refuses_and_explicit_attempt_returns_warning",
        |root| {
            let (mut session, doc, file) = setup(root);
            let damaged = damage(root, &file);
            let before = std::fs::read(damaged.artifact("input.json").unwrap()).unwrap();
            let check = required(session.save(doc, None).unwrap_err());
            assert!(check.evidence().conflict.analyses.is_empty());
            assert!(!check.evidence().is_complete());
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            assert!(session.can_undo(doc).unwrap());
            let check = required(
                session
                    .save_with_decision(doc, None, check.decide(SavePolicy::Cancel, false))
                    .unwrap_err(),
            );
            let outcome = session
                .save_with_decision(doc, None, check.decide(SavePolicy::Cancel, true))
                .unwrap();
            assert!(
                matches!(&outcome.warnings[..], [SaveWarning::EvidenceNotChecked { issues }] if !issues.is_empty())
            );
            assert_eq!(std::fs::read(&file).unwrap(), b">gene\nATGT\n");
            assert_eq!(
                std::fs::read(damaged.artifact("input.json").unwrap()).unwrap(),
                before
            );
        },
    );
}

#[test]
fn known_policy_and_incomplete_acknowledgement_are_independent() {
    isolated(
        "known_policy_and_incomplete_acknowledgement_are_independent",
        |root| {
            let (mut session, doc, file) = setup(root);
            let known = reference(&folder(root, "known"), &file);
            damage(root, &file);
            let check = session.check_save(doc, None).unwrap();
            assert_eq!(check.evidence().conflict.analyses.len(), 1);
            assert!(matches!(
                session.save_with_decision(doc, None, check.decide(SavePolicy::Cancel, true)),
                Err(Error::EvidenceConflict(_))
            ));
            assert_eq!(
                records::read_input(known.directory()).unwrap().state,
                basecraft_protocol::input_records::EvidenceState::Referenced
            );
            let check = session.check_save(doc, None).unwrap();
            let check = required(
                session
                    .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, false))
                    .unwrap_err(),
            );
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            session
                .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, true))
                .unwrap();
            assert_eq!(
                std::fs::read(known.directory().join("preserved-input")).unwrap(),
                ORIGINAL
            );
            assert_eq!(
                records::read_input(known.directory()).unwrap().state,
                basecraft_protocol::input_records::EvidenceState::Preserved
            );
        },
    );
}

#[test]
fn stale_document_target_issue_and_inventory_require_new_decisions() {
    isolated(
        "stale_document_target_issue_and_inventory_require_new_decisions",
        |root| {
            let (mut session, doc, file) = setup(root);
            let damaged = damage(root, &file);
            let check = session.check_save(doc, None).unwrap();
            session
                .apply(
                    doc,
                    Op::SeqReplace {
                        start: 2,
                        end: 3,
                        residues: "A".into(),
                        record: None,
                    },
                )
                .unwrap();
            assert!(matches!(
                session.save_with_decision(doc, None, check.decide(SavePolicy::Cancel, true)),
                Err(Error::SaveDecisionStale(_))
            ));
            let check = session.check_save(doc, None).unwrap();
            // Both invalid JSON strings produce the same parser message. Their
            // inspected bytes must nevertheless change the inventory revision.
            std::fs::write(damaged.artifact("input.json").unwrap(), b"damaged b").unwrap();
            assert!(matches!(
                session.save_with_decision(doc, None, check.decide(SavePolicy::Cancel, true)),
                Err(Error::SaveDecisionStale(_))
            ));
            let check = session.check_save(doc, None).unwrap();
            std::fs::remove_file(damaged.artifact("input.json").unwrap()).unwrap();
            assert!(matches!(
                session.save_with_decision(doc, None, check.decide(SavePolicy::Cancel, true)),
                Err(Error::SaveDecisionStale(_))
            ));
            // Save As changes the target identity even though the document is unchanged.
            let check = session.check_save(doc, None).unwrap();
            assert!(matches!(
                session.save_with_decision(
                    doc,
                    Some(&root.join("new.fasta")),
                    check.decide(SavePolicy::Cancel, true)
                ),
                Err(Error::SaveDecisionStale(_))
            ));
            let destination = root.join("destination.fasta");
            std::fs::write(&destination, ORIGINAL).unwrap();
            let check = session.check_save(doc, Some(&destination)).unwrap();
            std::fs::write(&destination, b">gene\nAAAA\n").unwrap();
            assert!(matches!(
                session.save_with_decision(
                    doc,
                    Some(&destination),
                    check.decide(SavePolicy::Cancel, true)
                ),
                Err(Error::SaveDecisionStale(_))
            ));
            // Equal bytes at the same path do not imply the same file identity.
            let check = session.check_save(doc, None).unwrap();
            let before_identity = records::stamp(&file).unwrap();
            let replacement = root.join("replacement.fasta");
            std::fs::write(&replacement, ORIGINAL).unwrap();
            std::fs::rename(&replacement, &file).unwrap();
            assert_ne!(records::stamp(&file).unwrap(), before_identity);
            assert!(matches!(
                session.save_with_decision(doc, None, check.decide(SavePolicy::Cancel, true)),
                Err(Error::SaveDecisionStale(_))
            ));
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
        },
    );
}

#[test]
fn save_as_rechecks_and_save_new_never_overwrites() {
    isolated("save_as_rechecks_and_save_new_never_overwrites", |root| {
        let (mut session, doc, file) = setup(root);
        damage(root, &file);
        let new = root.join("new.fasta");
        let check = session.check_save(doc, Some(&new)).unwrap();
        assert!(!check.evidence().is_complete());
        assert!(!new.exists());
        std::fs::write(&new, b"keep me").unwrap();
        let error = session
            .save_new_with_decision(doc, Some(&new), check.decide(SavePolicy::Cancel, true))
            .unwrap_err();
        assert!(!matches!(
            error,
            Error::SaveCheckRequired(_) | Error::EvidenceCheckIncomplete(_)
        ));
        assert_eq!(std::fs::read(&new).unwrap(), b"keep me");
        std::fs::remove_file(&new).unwrap();
        // A new file holds no recorded bytes: the unchecked store only warns.
        session.save_new(doc, Some(&new)).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
        assert_eq!(std::fs::read(&new).unwrap(), b">gene\nATGT\n");
    });
}

#[test]
fn preservation_and_changed_source_failures_are_not_waivable() {
    isolated(
        "preservation_and_changed_source_failures_are_not_waivable",
        |root| {
            let (mut session, doc, file) = setup(root);
            let known = reference(&folder(root, "known"), &file);
            damage(root, &file);
            std::fs::create_dir(known.directory().join("preserved-input")).unwrap();
            let check = session.check_save(doc, None).unwrap();
            let error = session
                .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, true))
                .unwrap_err();
            assert!(!matches!(
                error,
                Error::SaveCheckRequired(_)
                    | Error::SaveDecisionStale(_)
                    | Error::EvidenceCheckIncomplete(_)
            ));
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            std::fs::remove_dir(known.directory().join("preserved-input")).unwrap();
            // Replace by rename, as editors do; recovery keeps the old inode.
            let replacement = file.with_extension("tmp");
            std::fs::write(&replacement, b">external\nTTTT\n").unwrap();
            std::fs::rename(&replacement, &file).unwrap();
            let check = session.check_save(doc, None).unwrap();
            let error = session
                .save_with_decision(doc, None, check.decide(SavePolicy::BreakEvidence, true))
                .unwrap_err();
            assert!(!matches!(
                error,
                Error::SaveCheckRequired(_)
                    | Error::SaveDecisionStale(_)
                    | Error::EvidenceCheckIncomplete(_)
            ));
            assert_eq!(
                records::read_input(known.directory()).unwrap().state,
                basecraft_protocol::input_records::EvidenceState::Referenced
            );
            assert_eq!(std::fs::read(&file).unwrap(), b">external\nTTTT\n");
            assert_recovery(&mut session, root);
        },
    );
}

#[test]
fn partial_preservation_then_failed_save_remains_recoverable_and_retryable() {
    isolated(
        "partial_preservation_then_failed_save_remains_recoverable_and_retryable",
        |root| {
            let (mut session, doc, file) = setup(root);
            let known = reference(&folder(root, "known"), &file);
            damage(root, &file);
            let blocker = session.recovery_root().join("pending-save.json");
            std::fs::create_dir(&blocker).unwrap();
            let check = session.check_save(doc, None).unwrap();
            let error = session
                .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, true))
                .unwrap_err();
            assert!(!matches!(
                error,
                Error::SaveCheckRequired(_)
                    | Error::SaveDecisionStale(_)
                    | Error::EvidenceCheckIncomplete(_)
            ));
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            assert_eq!(
                std::fs::read(known.directory().join("preserved-input")).unwrap(),
                ORIGINAL
            );
            assert!(session.can_undo(doc).unwrap());
            std::fs::remove_dir(&blocker).unwrap();
            let check = session.check_save(doc, None).unwrap();
            assert!(check.evidence().conflict.analyses.is_empty());
            session
                .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, true))
                .unwrap();
            assert_eq!(
                std::fs::read(known.directory().join("preserved-input")).unwrap(),
                ORIGINAL
            );
            assert_eq!(std::fs::read(&file).unwrap(), b">gene\nATGT\n");
        },
    );
}

#[test]
fn advisory_check_does_not_create_durable_records() {
    isolated("advisory_check_does_not_create_durable_records", |root| {
        let file = root.join("gene.fasta");
        std::fs::write(&file, ORIGINAL).unwrap();
        let mut session = Session::recoverable(root).unwrap();
        let doc = session.open(&file).unwrap();
        let paths = AppPaths::discover().unwrap();
        assert!(!paths.state.exists());
        let check = session.check_save(doc, None).unwrap();
        assert!(check.evidence().is_complete());
        assert!(!paths.state.exists());
        assert!(!root.join(".basecraft").exists());
        assert!(!session.recovery_root().exists());
    });
}

#[test]
fn registered_dependency_after_query_refuses_stale_consent() {
    isolated(
        "registered_dependency_after_query_refuses_stale_consent",
        |root| {
            let (mut session, doc, file) = setup(root);
            damage(root, &file);
            let check = session.check_save(doc, None).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "late_registrar", "--nocapture"])
                .env("BASECRAFT_LATE_REGISTRATION", &file)
                .env("BASECRAFT_LATE_ANCHOR", root.join("late-result.txt"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let error = session
                .save_with_decision(doc, None, check.decide(SavePolicy::Cancel, true))
                .unwrap_err();
            assert!(matches!(error, Error::SaveDecisionStale(_)));
            let fresh = required(error);
            assert_eq!(fresh.evidence().conflict.analyses.len(), 1);
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            assert_recovery(&mut session, root);
        },
    );
}

#[test]
fn late_registrar() {
    let Some(source) = std::env::var_os("BASECRAFT_LATE_REGISTRATION") else {
        return;
    };
    let anchor = std::path::PathBuf::from(std::env::var_os("BASECRAFT_LATE_ANCHOR").unwrap());
    let source = std::path::PathBuf::from(source);
    records::reference_input_with_dependencies(
        &anchor,
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        &[],
        "late registration",
        None,
    )
    .unwrap();
}

#[test]
fn session_tokens_support_adapter_round_trip_and_cannot_be_replayed() {
    isolated(
        "session_tokens_support_adapter_round_trip_and_cannot_be_replayed",
        |root| {
            let (mut session, doc, file) = setup(root);
            let known = reference(&folder(root, "known"), &file);
            damage(root, &file);
            let check = session.check_save(doc, None).unwrap();
            let wire = serde_json::to_value(&check).unwrap();
            let token = wire["token"].as_str().unwrap();
            // Two adapter acknowledgements can be formed, but only one attempt
            // consumes this Session's pending receipt, even when that attempt fails.
            let first = session
                .decide_save(doc, None, token, SavePolicy::Cancel, true)
                .unwrap();
            let duplicate = session
                .decide_save(doc, None, token, SavePolicy::BreakEvidence, true)
                .unwrap();
            assert!(matches!(
                session.save_with_decision(doc, None, first),
                Err(Error::EvidenceConflict(_))
            ));
            assert!(matches!(
                session.save_with_decision(doc, None, duplicate),
                Err(Error::SaveDecisionStale(_))
            ));
            assert_eq!(
                records::read_input(known.directory()).unwrap().state,
                basecraft_protocol::input_records::EvidenceState::Referenced
            );
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            assert!(matches!(
                session.decide_save(
                    doc,
                    None,
                    "invented blanket consent",
                    SavePolicy::BreakEvidence,
                    true
                ),
                Err(Error::SaveDecisionStale(_))
            ));
            let mut other = Session::recoverable(root).unwrap();
            let other_doc = other.open(&file).unwrap();
            assert!(matches!(
                other.decide_save(other_doc, None, token, SavePolicy::BreakEvidence, true),
                Err(Error::SaveDecisionStale(_))
            ));
            // A new query supersedes the old token even with unchanged byte scope.
            let old = session.check_save(doc, None).unwrap();
            let fresh = session.check_save(doc, None).unwrap();
            assert_ne!(old.token(), fresh.token());
            assert!(matches!(
                session.save_with_decision(doc, None, old.decide(SavePolicy::BreakEvidence, true)),
                Err(Error::SaveDecisionStale(_))
            ));
        },
    );
}

#[test]
fn incomplete_inspection_keeps_descendants_ownership_and_backing() {
    isolated(
        "incomplete_inspection_keeps_descendants_ownership_and_backing",
        |root| {
            let (mut session, doc, file) = setup(root);
            let anchor = folder(root, "analysis");
            let ledger = reference(&anchor, &file);
            let unrelated = root.join("other.fasta");
            std::fs::write(&unrelated, ORIGINAL).unwrap();
            let child = records::reference_input_with_dependencies(
                &anchor.join("child.txt"),
                &unrelated,
                &basecraft_io::hash_file(&unrelated).unwrap(),
                &[ledger.directory().into()],
                "child",
                None,
            )
            .unwrap();
            let grandchild = records::reference_input_with_dependencies(
                &anchor.join("grandchild.txt"),
                &unrelated,
                &basecraft_io::hash_file(&unrelated).unwrap(),
                &[child.directory().into()],
                "grandchild",
                None,
            )
            .unwrap();
            let mut input = records::read_input(ledger.directory()).unwrap();
            input.analysis_run =
                Some(records::relative(ledger.directory(), child.directory()).unwrap());
            records::write_json(&ledger.artifact("input.json").unwrap(), &input).unwrap();
            // Damage in this very store must not discard any successfully read
            // ledger, known descendant, logical owner or backing decision.
            let damaged = reference(&anchor, &unrelated);
            std::fs::write(damaged.artifact("input.json").unwrap(), b"damaged").unwrap();
            let check = session.check_save(doc, None).unwrap();
            assert!(!check.evidence().is_complete());
            let analyses = &check.evidence().conflict.analyses;
            assert_eq!(analyses.len(), 2);
            let owner = analyses
                .iter()
                .find(|a| a.run == child.directory())
                .unwrap();
            assert!(
                owner
                    .backing_analyses
                    .iter()
                    .any(|a| a.run == ledger.directory() && a.direct)
            );
            assert!(
                owner
                    .backing_analyses
                    .iter()
                    .any(|a| a.run == child.directory())
            );
            assert!(
                analyses
                    .iter()
                    .any(|a| a.run == grandchild.directory() && !a.direct)
            );
            session
                .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, true))
                .unwrap();
            assert_eq!(
                std::fs::read(ledger.directory().join("preserved-input")).unwrap(),
                ORIGINAL
            );
        },
    );
}

#[test]
fn damaged_display_and_registry_entries_do_not_hide_known_conflicts() {
    isolated(
        "damaged_display_and_registry_entries_do_not_hide_known_conflicts",
        |root| {
            let (mut session, doc, file) = setup(root);
            let known = reference(&folder(root, "known"), &file);
            std::fs::write(known.artifact("run.json").unwrap(), b"damaged").unwrap();
            let registry = AppPaths::discover()
                .unwrap()
                .state
                .join("analysis-stores.json");
            let mut value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&registry).unwrap()).unwrap();
            value["stores"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!("relative invalid entry"));
            std::fs::write(&registry, serde_json::to_vec(&value).unwrap()).unwrap();
            let check = required(session.save(doc, None).unwrap_err());
            assert_eq!(check.evidence().conflict.analyses.len(), 1);
            // The damaged registry entry is not read by saves; it hides nothing.
            assert_ne!(check.evidence().issues.len(), 0);
            assert_eq!(check.evidence().conflict.analyses[0].run, known.directory());
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            assert!(session.can_undo(doc).unwrap());
        },
    );
}

#[test]
fn review_wrong_document_attempt_consumes_original_receipt() {
    isolated(
        "review_wrong_document_attempt_consumes_original_receipt",
        |root| {
            let (mut session, doc, file) = setup(root);
            damage(root, &file);
            let second_file = root.join("second.fasta");
            std::fs::write(&second_file, ORIGINAL).unwrap();
            let second = session.open(&second_file).unwrap();
            let check = session.check_save(doc, None).unwrap();
            let duplicate = check.clone().decide(SavePolicy::Cancel, true);
            let wrong =
                session.save_with_decision(second, None, check.decide(SavePolicy::Cancel, true));
            assert!(matches!(wrong, Err(Error::SaveDecisionStale(_))));
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            let replay = session.save_with_decision(doc, None, duplicate);
            assert!(
                matches!(replay, Err(Error::SaveDecisionStale(_))),
                "receipt remained usable after wrong-document attempt: {replay:?}"
            );
        },
    );
}

#[test]
fn invalid_document_attempt_consumes_originating_receipt() {
    isolated(
        "invalid_document_attempt_consumes_originating_receipt",
        |root| {
            let (mut session, doc, file) = setup(root);
            damage(root, &file);
            let check = session.check_save(doc, None).unwrap();
            let duplicate = check.clone().decide(SavePolicy::Cancel, true);
            let invalid = session.save_with_decision(
                DocId(usize::MAX),
                None,
                check.decide(SavePolicy::Cancel, true),
            );
            assert!(matches!(invalid, Err(Error::UnknownDocument { .. })));
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            assert!(session.can_undo(doc).unwrap());
            assert!(matches!(
                session.save_with_decision(doc, None, duplicate),
                Err(Error::SaveDecisionStale(_))
            ));
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
        },
    );
}

#[test]
fn invalid_document_attempt_does_not_consume_newer_receipt() {
    isolated(
        "invalid_document_attempt_does_not_consume_newer_receipt",
        |root| {
            let (mut session, doc, file) = setup(root);
            damage(root, &file);
            let second_file = root.join("second.fasta");
            std::fs::write(&second_file, ORIGINAL).unwrap();
            let second = session.open(&second_file).unwrap();
            let old = session.check_save(doc, None).unwrap();
            let newer = session.check_save(doc, None).unwrap();
            assert!(matches!(
                session.save_with_decision(
                    second,
                    None,
                    old.clone().decide(SavePolicy::Cancel, true)
                ),
                Err(Error::SaveDecisionStale(_))
            ));
            assert_eq!(std::fs::read(&second_file).unwrap(), ORIGINAL);
            assert!(matches!(
                session.save_with_decision(
                    DocId(usize::MAX),
                    None,
                    old.decide(SavePolicy::Cancel, true)
                ),
                Err(Error::UnknownDocument { .. })
            ));
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            let outcome = session
                .save_with_decision(doc, None, newer.decide(SavePolicy::Cancel, true))
                .unwrap();
            assert_eq!(outcome.warnings.len(), 1);
            assert_eq!(std::fs::read(&file).unwrap(), b">gene\nATGT\n");
        },
    );
}

#[test]
fn foreign_receipt_cannot_consume_local_or_originating_session_authority() {
    isolated(
        "foreign_receipt_cannot_consume_local_or_originating_session_authority",
        |root| {
            let (mut origin, doc, file) = setup(root);
            damage(root, &file);
            let check = origin.check_save(doc, None).unwrap();
            let duplicate = check.clone().decide(SavePolicy::Cancel, true);
            let other_root = folder(root, "other-session");
            let (mut other, other_doc, other_file) = setup(&other_root);
            let rejected = other
                .save_with_decision(
                    other_doc,
                    None,
                    check.clone().decide(SavePolicy::Cancel, true),
                )
                .unwrap_err();
            assert!(matches!(rejected, Error::SaveDecisionStale(_)));
            let local = required(rejected);
            assert!(matches!(
                other.save_with_decision(
                    DocId(usize::MAX),
                    None,
                    check.decide(SavePolicy::Cancel, true)
                ),
                Err(Error::UnknownDocument { .. })
            ));
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            assert_eq!(std::fs::read(&other_file).unwrap(), ORIGINAL);
            other
                .save_with_decision(other_doc, None, local.decide(SavePolicy::Cancel, true))
                .unwrap();
            origin.save_with_decision(doc, None, duplicate).unwrap();
            assert_eq!(std::fs::read(&file).unwrap(), b">gene\nATGT\n");
            assert_eq!(std::fs::read(&other_file).unwrap(), b">gene\nATGT\n");
        },
    );
}

#[test]
fn foreign_rejection_supersedes_requested_receipt_without_spending_origin() {
    isolated(
        "foreign_rejection_supersedes_requested_receipt_without_spending_origin",
        |root| {
            let (mut origin, doc, file) = setup(root);
            damage(root, &file);
            let origin_check = origin.check_save(doc, None).unwrap();
            let other_root = folder(root, "requested-owner");
            let (mut other, other_doc, other_file) = setup(&other_root);
            let superseded = other.check_save(other_doc, None).unwrap();
            let fresh = required(
                other
                    .save_with_decision(
                        other_doc,
                        None,
                        origin_check.clone().decide(SavePolicy::Cancel, true),
                    )
                    .unwrap_err(),
            );
            assert_ne!(superseded.token(), fresh.token());
            // Rejecting the foreign receipt published a new requested-owner
            // check. This is supersession, not consumption of the origin.
            let refreshed = required(
                other
                    .save_with_decision(
                        other_doc,
                        None,
                        superseded.decide(SavePolicy::Cancel, true),
                    )
                    .unwrap_err(),
            );
            assert_ne!(fresh.token(), refreshed.token());
            assert_eq!(std::fs::read(&file).unwrap(), ORIGINAL);
            assert_eq!(std::fs::read(&other_file).unwrap(), ORIGINAL);
            // Moving the live owner retains its one pending receipt.
            let mut moved = other;
            moved
                .save_with_decision(other_doc, None, refreshed.decide(SavePolicy::Cancel, true))
                .unwrap();
            origin
                .save_with_decision(doc, None, origin_check.decide(SavePolicy::Cancel, true))
                .unwrap();
            assert_eq!(std::fs::read(&file).unwrap(), b">gene\nATGT\n");
            assert_eq!(std::fs::read(&other_file).unwrap(), b">gene\nATGT\n");
        },
    );
}
