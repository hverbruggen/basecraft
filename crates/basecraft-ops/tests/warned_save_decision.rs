//! One-attempt incomplete-evidence decisions at the authoritative save boundary.
use basecraft_ops::{
    DocId, Error, Session,
    analysis_store::ArtifactRun,
    input_evidence::{self, SavePolicy},
    replay,
    session::{SaveCheck, SaveWarning},
};
use basecraft_platform::paths::AppPaths;
use basecraft_protocol::Op;
use std::{path::Path, process::Command};

const ORIGINAL: &[u8] = b">gene\nACGT\n";

// Reuse the budget/kind-conformance child-process pattern. Command::env selects
// the profile before any AppPaths/Session initialization; no process-global env
// mutation or serial lock is needed in the parallel parent harness.
fn isolated(name: &str, test: impl FnOnce(&Path)) {
    if std::env::var("BASECRAFT_DECISION_CHILD").as_deref() != Ok(name) {
        let root = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("BASECRAFT_DECISION_CHILD", name)
            .env("BASECRAFT_DECISION_ROOT", root.path())
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
            .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
            .env("BASECRAFT_SCRATCH_DIR", root.path().join("scratch"))
            .env("TMPDIR", root.path())
            .env("TMP", root.path())
            .env("TEMP", root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("BASECRAFT_DECISION_ROOT").unwrap());
    assert_eq!(
        AppPaths::discover().unwrap().state,
        root.join("profile/state")
    );
    test(&root);
}

fn folder(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn reference(root: &Path, source: &Path) -> ArtifactRun {
    input_evidence::reference_input_with_dependencies(
        &root.join("result.txt"),
        source,
        &basecraft_io::hash_file(source).unwrap(),
        &[],
        "save protection regression",
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
    std::fs::write(run.artifact("input.json").unwrap(), b"damaged a").unwrap();
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
    session.flush().unwrap();
    let outputs = replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
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
            assert!(session.can_undo(doc));
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
                input_evidence::read_input(known.directory()).unwrap().state,
                "referenced"
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
                input_evidence::read_input(known.directory()).unwrap().state,
                "preserved"
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
        let check = required(session.save_new(doc, Some(&new)).unwrap_err());
        let outcome = session
            .save_new_with_decision(doc, Some(&new), check.decide(SavePolicy::Cancel, true))
            .unwrap();
        assert_eq!(outcome.warnings.len(), 1);
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
            std::fs::write(&file, b">external\nTTTT\n").unwrap();
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
                input_evidence::read_input(known.directory()).unwrap().state,
                "referenced"
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
            assert!(session.can_undo(doc));
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
    input_evidence::reference_input_with_dependencies(
        &anchor,
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        &[],
        "late registration",
    )
    .unwrap();
}
