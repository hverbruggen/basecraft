//! Retained recovery authority and publication, using isolated real files.
use basecraft_ops::{
    Error, Session,
    input_evidence::SavePolicy,
    records::{self, RecoveryMode},
};
use basecraft_platform::files::{OrdinaryKind, object_identity};
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
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let check = attempt.check(0).unwrap();
    let stage = check.target().parent().unwrap().to_path_buf();
    attempt.advance(0, None).unwrap();
    let first = object_identity(check.target(), OrdinaryKind::File).unwrap();
    attempt.advance(1, None).unwrap();
    let complete = attempt.publish().unwrap();
    assert_eq!(complete.outputs.len(), 2);
    assert_eq!(
        object_identity(&complete.outputs[0], OrdinaryKind::File).unwrap(),
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
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let path = attempt.check(0).unwrap().target().to_path_buf();
    attempt.advance(0, None).unwrap();
    let first = object_identity(&path, OrdinaryKind::File).unwrap();
    damage();
    let second = required(attempt.advance(1, None).unwrap_err());
    attempt
        .advance(1, Some(second.decide(SavePolicy::Cancel, true)))
        .unwrap();
    let fresh = required(attempt.publish().unwrap_err());
    let status = attempt.status().unwrap();
    assert_eq!(status.phase, records::RecoveryPhase::NeedsFinalization);
    assert!(status.documents.iter().all(|document| document.written));
    assert_eq!(
        serde_json::to_value(&status.warnings).unwrap(),
        serde_json::to_value(attempt.warnings()).unwrap()
    );
    assert!(!status.warnings.is_empty());
    assert!(!destination.exists());
    let refused = required(
        attempt
            .advance(0, Some(fresh.decide(SavePolicy::Cancel, false)))
            .unwrap_err(),
    );
    attempt
        .advance(0, Some(refused.decide(SavePolicy::Cancel, true)))
        .unwrap();
    assert_eq!(object_identity(&path, OrdinaryKind::File).unwrap(), first);
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
    let mut a = records::prepare_recovery(
        session.recovery_root(),
        &root.join("a"),
        RecoveryMode::Strict,
    )
    .unwrap();
    let mut b = records::prepare_recovery(
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
    let mut attempt = records::prepare_recovery(
        session.recovery_root(),
        &root.join("out"),
        RecoveryMode::Strict,
    )
    .unwrap();
    let target = attempt.check(0).unwrap().target().to_path_buf();
    let stage = target.parent().unwrap().to_path_buf();
    attempt.advance(0, None).unwrap();
    let bytes = std::fs::read(&target).unwrap();
    let retained = root.join("retained-old-staged-file");
    std::fs::rename(&target, &retained).unwrap();
    std::fs::write(&target, &bytes).unwrap();
    assert_ne!(
        object_identity(&target, OrdinaryKind::File).unwrap(),
        object_identity(&retained, OrdinaryKind::File).unwrap()
    );
    assert!(attempt.publish().is_err());
    attempt.cancel();
    assert!(!stage.exists());
    assert!(!root.join("out").exists());
    assert_eq!(
        std::fs::read(root.join("source/0.fasta")).unwrap(),
        b">gene\nACGT\n"
    );
    assert_ne!(
        basecraft_ops::log::read_all(session.recovery_root()).unwrap(),
        [] as [basecraft_protocol::LogEntry; 0]
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
    for kind in 0..3 {
        let destination = root.join(format!("competitor-{kind}"));
        let mut attempt =
            records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
                .unwrap();
        attempt.advance(0, None).unwrap();
        match kind {
            0 => std::fs::write(&destination, b"competitor").unwrap(),
            1 => std::fs::create_dir(&destination).unwrap(),
            _ => {
                std::fs::create_dir(&destination).unwrap();
                std::fs::write(destination.join("owned"), b"competitor").unwrap();
            }
        }
        assert!(attempt.publish().is_err());
        assert!(std::fs::symlink_metadata(&destination).is_ok());
        assert!(
            records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
                .is_err()
        );
    }
    let parent = root.join("parent");
    std::fs::create_dir(&parent).unwrap();
    let mut attempt = records::prepare_recovery(
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

fn reference(anchor: &Path, source: &Path) -> basecraft_ops::analysis_store::ArtifactRun {
    std::fs::create_dir_all(anchor.parent().unwrap()).unwrap();
    basecraft_ops::input_evidence::reference_input_with_dependencies(
        anchor,
        source,
        &basecraft_io::hash_file(source).unwrap(),
        &[],
        "recovery test",
    )
    .unwrap()
}

#[test]
fn stage_and_final_policies_validate_all_groups_before_any_mutation() {
    let Some(root) =
        isolated_profile::enter("stage_and_final_policies_validate_all_groups_before_any_mutation")
    else {
        return;
    };
    let session = source(&root, 1);
    let destination = root.join("recovered");
    std::fs::create_dir(&destination).unwrap();
    let final_file = destination.join("0.fasta");
    std::fs::write(&final_file, b">old\nAAAA\n").unwrap();
    let final_run = reference(&root.join("final-analysis/result"), &final_file);
    std::fs::remove_file(&final_file).unwrap();
    std::fs::remove_dir(&destination).unwrap();
    let mut attempt =
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let stage_file = attempt.check(0).unwrap().target().to_path_buf();
    let stage_run = reference(&root.join("stage-analysis/result"), &stage_file);
    let owner = reference(&root.join("owner/result"), &root.join("source/0.fasta"));
    for run in [&stage_run, &final_run] {
        let mut input = basecraft_ops::input_evidence::read_input(run.directory()).unwrap();
        input.analysis_run = Some(
            basecraft_ops::input_evidence::relative(run.directory(), owner.directory()).unwrap(),
        );
        basecraft_ops::input_evidence::write_json(&run.directory().join("input.json"), &input)
            .unwrap();
    }
    let original = std::fs::read(stage_run.directory().join("input.json")).unwrap();
    let check = attempt.check(0).unwrap();
    let groups: Vec<_> = check.evidence_groups().collect();
    assert_eq!(groups.len(), 2);
    assert!(
        groups[0]
            .conflict
            .options
            .contains(&SavePolicy::PreserveCopy)
    );
    assert!(
        !groups[1]
            .conflict
            .options
            .contains(&SavePolicy::PreserveCopy)
    );
    assert!(!groups[0].is_complete());
    assert!(groups[1].is_complete());
    assert_eq!(groups[0].conflict.analyses[0].run, owner.directory());
    assert_eq!(groups[1].conflict.analyses[0].run, owner.directory());
    assert!(
        groups[0].conflict.analyses[0]
            .backing_analyses
            .iter()
            .any(|backing| backing.run == stage_run.directory() && backing.direct)
    );
    assert!(
        groups[1].conflict.analyses[0]
            .backing_analyses
            .iter()
            .any(|backing| backing.run == final_run.directory() && backing.direct)
    );
    assert!(matches!(
        attempt.advance(0, Some(check.decide(SavePolicy::Cancel, true))),
        Err(Error::EvidenceConflict(_))
    ));
    let check = attempt.check(0).unwrap();
    assert!(
        attempt
            .advance(0, Some(check.decide(SavePolicy::PreserveCopy, true)))
            .is_err()
    );
    assert_eq!(
        std::fs::read(stage_run.directory().join("input.json")).unwrap(),
        original
    );
    assert!(!stage_run.directory().join("preserved-input").exists());
    assert!(attempt.evidence_actions().unwrap().is_empty());
    let check = attempt.check(0).unwrap();
    attempt
        .advance(0, Some(check.decide(SavePolicy::BreakEvidence, true)))
        .unwrap();
    let completed = match attempt.publish() {
        Ok(completed) => completed,
        Err(error) => {
            let check = required(error);
            attempt
                .advance(0, Some(check.decide(SavePolicy::Cancel, true)))
                .unwrap();
            attempt.publish().unwrap()
        }
    };
    assert_eq!(completed.evidence_actions.len(), 2);
    assert_eq!(
        serde_json::to_value(attempt.status().unwrap().evidence_actions).unwrap(),
        serde_json::to_value(&completed.evidence_actions).unwrap()
    );
    assert!(
        completed
            .evidence_actions
            .iter()
            .all(|action| action.completed)
    );
    for run in [stage_run, final_run] {
        assert_eq!(
            basecraft_ops::input_evidence::read_input(run.directory())
                .unwrap()
                .state,
            basecraft_protocol::input_records::EvidenceState::Broken
        );
    }
}

#[test]
fn nested_delimitation_registers_only_final_stores_and_reopens() {
    let Some(root) =
        isolated_profile::enter("nested_delimitation_registers_only_final_stores_and_reopens")
    else {
        return;
    };
    let original = root.join("source");
    std::fs::create_dir_all(original.join("nested")).unwrap();
    let external_input = root.join("external.fasta");
    std::fs::write(&external_input, b">external\nAAAA\n").unwrap();
    let external = reference(&root.join("external-store/result"), &external_input);
    let ledger_path = external.directory().join("input.json");
    let mut ledger: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&ledger_path).unwrap()).unwrap();
    ledger["future_extension"] = serde_json::json!({"keep": [1, 2, 3]});
    std::fs::write(&ledger_path, serde_json::to_vec(&ledger).unwrap()).unwrap();
    let ledger_bytes = std::fs::read(&ledger_path).unwrap();
    let registry_before = std::fs::read(registry()).unwrap();
    let mut source = Session::recoverable(&original).unwrap();
    for name in ["a.spart", "nested/b.spart.xml"] {
        let path = original.join(name);
        // Use the existing exchange writers for each supported encoding.
        let mut work = basecraft_io::spart::parse_text(include_str!(
            "../../../testdata/fixtures/delimitation/manual.spart"
        ))
        .unwrap();
        work.exchange_fields
            .insert("retained-unknown".into(), "keep".into());
        work.exchange_fields.insert(
            "basecraft_alignment_evidence_run".into(),
            external.directory().to_str().unwrap().into(),
        );
        basecraft_io::write_atomic(&path, |w| {
            basecraft_ops::files::write_delimitation(&work, name.ends_with("xml"), w)
        })
        .unwrap();
        let sidecar = basecraft_ops::files::tree_sidecar(&path);
        basecraft_io::write_atomic(&sidecar, |w| {
            basecraft_ops::files::write_delimitation_sidecar_at(
                &work,
                &basecraft_io::hash_file(&path).map_err(std::io::Error::other)?,
                &path,
                w,
            )
        })
        .unwrap();
        assert_eq!(
            basecraft_ops::files::load_delimitation(&path)
                .unwrap()
                .exchange_fields["retained-unknown"],
            "keep"
        );
        let doc = source.open(&path).unwrap();
        source
            .apply(
                doc,
                Op::DelimitationEdit {
                    edit: Box::new(basecraft_protocol::delimitation::DelimitationEdit::Remove {
                        hypotheses: ["partition-3".into()].into(),
                    }),
                },
            )
            .unwrap();
    }
    source.flush().unwrap();
    let destination = root.join("final");
    let mut attempt =
        records::prepare_recovery(source.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let stage = attempt
        .check(0)
        .unwrap()
        .target()
        .parent()
        .unwrap()
        .to_path_buf();
    attempt.advance(0, None).unwrap();
    attempt.advance(1, None).unwrap();
    assert_eq!(std::fs::read(registry()).unwrap(), registry_before);
    // A second owned store can alter the earlier evidence inventory. Refresh
    // only the requested finalization check; never run the writers a second time.
    let complete = loop {
        match attempt.publish() {
            Ok(done) => break done,
            Err(error) => {
                let check = required(error);
                let document = attempt.document_for_check(&check).unwrap();
                attempt
                    .advance(document, Some(check.decide(SavePolicy::Cancel, false)))
                    .unwrap();
            }
        }
    };
    assert_eq!(complete.registered_stores.len(), 2);
    let registered = std::fs::read_to_string(registry()).unwrap();
    assert!(!registered.contains(stage.to_str().unwrap()));
    assert!(!stage.exists());
    for file in complete.outputs {
        let work = basecraft_ops::files::load_delimitation(&file).unwrap();
        assert_eq!(work.hypotheses.len(), 2);
        assert_eq!(work.exchange_fields["retained-unknown"], "keep");
        let side = basecraft_ops::files::tree_sidecar(&file);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(side).unwrap()).unwrap();
        assert_eq!(value["path_base"], file.parent().unwrap().to_str().unwrap());
        let run = basecraft_ops::analysis_store::ArtifactRun::for_result(&file)
            .unwrap()
            .unwrap();
        let input = basecraft_ops::input_evidence::read_input(&run).unwrap();
        let upstream = serde_json::to_value(&input.upstream).unwrap();
        let upstream = upstream.as_array().unwrap();
        assert!(upstream.iter().any(|record| {
            run.join(record["bundle"].as_str().unwrap())
                .canonicalize()
                .unwrap()
                == external.directory()
        }));
        assert_eq!(
            std::fs::read(run.join("submitted.fasta")).unwrap(),
            std::fs::read(&file).unwrap()
        );
        assert_eq!(std::fs::read(&ledger_path).unwrap(), ledger_bytes);
    }
}

#[test]
fn damaged_journal_requires_explicit_prefix_and_preserves_bytes() {
    use std::io::Write;
    let Some(root) =
        isolated_profile::enter("damaged_journal_requires_explicit_prefix_and_preserves_bytes")
    else {
        return;
    };
    let session = source(&root, 1);
    let host = std::fs::read_dir(basecraft_ops::log::log_dir(session.recovery_root()))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let journal = std::fs::read_dir(host)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&journal)
        .unwrap()
        .write_all(b"{torn")
        .unwrap();
    let bytes = std::fs::read(&journal).unwrap();
    assert!(records::recover(session.recovery_root(), &root.join("strict")).is_err());
    assert!(!root.join("strict").exists());
    let mut prefix = records::prepare_recovery(
        session.recovery_root(),
        &root.join("prefix"),
        RecoveryMode::VerifiedPrefix,
    )
    .unwrap();
    assert!(prefix.damage().is_some());
    assert_eq!(prefix.status().unwrap().damage.as_deref(), prefix.damage());
    prefix.advance(0, None).unwrap();
    let completed = prefix.publish().unwrap();
    assert!(completed.damage.is_some());
    assert_eq!(
        std::fs::read(&completed.outputs[0]).unwrap(),
        b">gene\nATGT\n"
    );
    assert_eq!(std::fs::read(journal).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn permission_and_preservation_failures_spend_receipts_without_publication() {
    use std::os::unix::fs::PermissionsExt;
    let Some(root) = isolated_profile::enter(
        "permission_and_preservation_failures_spend_receipts_without_publication",
    ) else {
        return;
    };
    let session = source(&root, 1);
    let destination = root.join("out");
    let mut attempt =
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let check = attempt.check(0).unwrap();
    let stage_file = check.target().to_path_buf();
    let stage = stage_file.parent().unwrap();
    std::fs::set_permissions(stage, std::fs::Permissions::from_mode(0o500)).unwrap();
    let failed = attempt.advance(0, Some(check.clone().decide(SavePolicy::Cancel, false)));
    std::fs::set_permissions(stage, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(failed, Err(Error::Format(_) | Error::Io(_))));
    assert!(!destination.exists());
    assert!(matches!(
        attempt.advance(0, Some(check.decide(SavePolicy::Cancel, false))),
        Err(Error::SaveDecisionStale(_))
    ));
    let run = reference(&root.join("known/result"), &stage_file);
    let original = std::fs::read(&stage_file).unwrap();
    let check = attempt.check(0).unwrap();
    std::fs::set_permissions(run.directory(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let failed = attempt.advance(
        0,
        Some(check.clone().decide(SavePolicy::PreserveCopy, false)),
    );
    std::fs::set_permissions(run.directory(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(failed, Err(Error::Format(_) | Error::Io(_))));
    assert_eq!(std::fs::read(&stage_file).unwrap(), original);
    assert!(!destination.exists());
    assert!(
        !attempt
            .evidence_actions()
            .unwrap()
            .last()
            .unwrap()
            .completed
    );
    assert!(matches!(
        attempt.advance(0, Some(check.decide(SavePolicy::PreserveCopy, false))),
        Err(Error::SaveDecisionStale(_))
    ));
    let check = attempt.check(0).unwrap();
    attempt
        .advance(0, Some(check.decide(SavePolicy::PreserveCopy, false)))
        .unwrap();
    attempt.publish().unwrap();
    assert_eq!(
        std::fs::read(run.directory().join("preserved-input")).unwrap(),
        original
    );
}

#[test]
fn removal_of_shared_backing_ledgers_across_target_groups_is_once() {
    let Some(root) =
        isolated_profile::enter("removal_of_shared_backing_ledgers_across_target_groups_is_once")
    else {
        return;
    };
    let session = source(&root, 1);
    let destination = root.join("out");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("0.fasta"), b">old\nAAAA\n").unwrap();
    let final_run = reference(&root.join("final/result"), &destination.join("0.fasta"));
    std::fs::remove_file(destination.join("0.fasta")).unwrap();
    std::fs::remove_dir(&destination).unwrap();
    let mut attempt =
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let stage_run = reference(
        &root.join("stage/result"),
        attempt.check(0).unwrap().target(),
    );
    let owner = reference(&root.join("owner/result"), &root.join("source/0.fasta"));
    for run in [&stage_run, &final_run] {
        let mut input = basecraft_ops::input_evidence::read_input(run.directory()).unwrap();
        input.analysis_run = Some(
            basecraft_ops::input_evidence::relative(run.directory(), owner.directory()).unwrap(),
        );
        basecraft_ops::input_evidence::write_json(&run.directory().join("input.json"), &input)
            .unwrap();
    }
    let check = attempt.check(0).unwrap();
    attempt
        .advance(0, Some(check.decide(SavePolicy::RemoveAnalysis, true)))
        .unwrap();
    let complete = match attempt.publish() {
        Ok(complete) => complete,
        Err(error) => {
            let check = required(error);
            attempt
                .advance(0, Some(check.decide(SavePolicy::Cancel, true)))
                .unwrap();
            attempt.publish().unwrap()
        }
    };
    assert_eq!(complete.evidence_actions.len(), 2);
    assert!(complete.evidence_actions.iter().all(|a| a.completed));
    for run in [stage_run, final_run, owner] {
        assert!(!run.directory().exists());
        let trash = run
            .directory()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("trash");
        assert_eq!(std::fs::read_dir(trash).unwrap().count(), 1);
    }
}

#[test]
fn new_unsaved_sequence_has_final_only_issue_group_and_explicit_recovery() {
    let Some(root) = isolated_profile::enter(
        "new_unsaved_sequence_has_final_only_issue_group_and_explicit_recovery",
    ) else {
        return;
    };
    let mut source = Session::recoverable(&root).unwrap();
    source.create_sequence("new", "ACGT").unwrap();
    source.flush().unwrap();
    let mut attempt = records::prepare_recovery(
        source.recovery_root(),
        &root.join("out"),
        RecoveryMode::Strict,
    )
    .unwrap();
    let stage_target = attempt.check(0).unwrap().target().to_path_buf();
    assert!(!stage_target.exists());
    let original = root.join("original.fasta");
    std::fs::write(&original, b">gene\nACGT\n").unwrap();
    let run = reference(&root.join("external/result"), &original);
    let mut input = basecraft_ops::input_evidence::read_input(run.directory()).unwrap();
    input.source.as_mut().unwrap().path =
        basecraft_ops::input_evidence::relative(run.directory(), &stage_target).unwrap();
    basecraft_ops::input_evidence::write_json(&run.directory().join("input.json"), &input).unwrap();
    let check = attempt.check(0).unwrap();
    let groups: Vec<_> = check.evidence_groups().collect();
    assert!(groups[0].is_complete());
    assert!(!groups[1].is_complete());
    assert!(!groups[0].conflict.analyses.is_empty());
    assert!(groups[1].conflict.analyses.is_empty());
    assert!(matches!(
        attempt.advance(0, Some(check.decide(SavePolicy::Cancel, true))),
        Err(Error::EvidenceConflict(_))
    ));
    let check = attempt.check(0).unwrap();
    attempt
        .advance(0, Some(check.decide(SavePolicy::BreakEvidence, true)))
        .unwrap();
    let complete = match attempt.publish() {
        Ok(complete) => complete,
        Err(error) => {
            let check = required(error);
            attempt
                .advance(0, Some(check.decide(SavePolicy::Cancel, true)))
                .unwrap();
            attempt.publish().unwrap()
        }
    };
    assert_eq!(complete.outputs.len(), 1);
    let mut reopened = Session::unlogged(&root);
    let doc = reopened.open(&complete.outputs[0]).unwrap();
    assert_eq!(reopened.state(doc).unwrap().len, 4);
}

#[test]
fn status_is_read_only_and_tracks_document_progress_without_slot_ids() {
    use records::RecoveryPhase;
    let Some(root) = isolated_profile::enter(
        "status_is_read_only_and_tracks_document_progress_without_slot_ids",
    ) else {
        return;
    };
    let mut session = source(&root, 2);
    // Reopen a key so the originating Slot id differs from the attempt index.
    let reopened = session.open(&root.join("source/0.fasta")).unwrap();
    session
        .apply(
            reopened,
            Op::SeqReplace {
                start: 1,
                end: 2,
                residues: "T".into(),
            },
        )
        .unwrap();
    session.flush().unwrap();
    let destination = root.join("out");
    let mut attempt =
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let check = attempt.check(0).unwrap();
    assert_ne!(check.document(), 0);
    let initial = attempt.status().unwrap();
    assert_eq!(initial.destination, destination);
    assert_eq!(initial.phase, RecoveryPhase::PendingWrite);
    assert_eq!(initial.documents.len(), 2);
    for (index, document) in initial.documents.iter().enumerate() {
        assert_eq!(document.index, index);
        assert_eq!(document.relative_key, format!("{index}.fasta"));
        assert_eq!(document.phase, RecoveryPhase::PendingWrite);
        assert!(!document.written);
    }
    let stage = check.target().parent().unwrap();
    let parked = root.join("unavailable-stage");
    let registry_before = std::fs::read(registry()).ok();
    let journal_before =
        serde_json::to_value(basecraft_ops::log::read_all(session.recovery_root()).unwrap())
            .unwrap();
    std::fs::rename(stage, &parked).unwrap();
    for _ in 0..2 {
        // A validation/check/hash would fail at the missing stage. Snapshot
        // reads only stored fields and must leave the pending receipt alone.
        assert_eq!(
            serde_json::to_value(attempt.status().unwrap()).unwrap(),
            serde_json::to_value(&initial).unwrap()
        );
    }
    std::fs::rename(&parked, stage).unwrap();
    assert_eq!(std::fs::read(registry()).ok(), registry_before);
    assert_eq!(
        serde_json::to_value(basecraft_ops::log::read_all(session.recovery_root()).unwrap())
            .unwrap(),
        journal_before
    );
    let decision = attempt
        .decide(0, check.token(), SavePolicy::Cancel, false)
        .unwrap();
    attempt.advance(0, Some(decision)).unwrap();
    let partial = attempt.status().unwrap();
    assert_eq!(partial.phase, RecoveryPhase::PendingWrite);
    assert_eq!(partial.documents[0].phase, RecoveryPhase::NeedsFinalization);
    assert!(partial.documents[0].written);
    assert_eq!(partial.documents[1].phase, RecoveryPhase::PendingWrite);
    assert!(!partial.documents[1].written);
    attempt.advance(1, None).unwrap();
    assert_eq!(
        attempt.status().unwrap().phase,
        RecoveryPhase::NeedsFinalization
    );
    let completion = attempt.publish().unwrap();
    let mut terminal = attempt.status().unwrap();
    assert_eq!(terminal.phase, RecoveryPhase::Published);
    assert_eq!(terminal.destination, completion.destination);
    assert!(
        terminal
            .documents
            .iter()
            .all(|document| document.written && document.phase == RecoveryPhase::Published)
    );
    terminal.documents[0].written = false;
    terminal.documents[0].relative_key = "changed snapshot".into();
    assert!(attempt.status().unwrap().documents[0].written);
    assert_eq!(
        attempt.status().unwrap().documents[0].relative_key,
        "0.fasta"
    );
}

#[cfg(unix)]
#[test]
fn unix_dangling_destination_is_occupied() {
    let Some(root) = isolated_profile::enter("unix_dangling_destination_is_occupied") else {
        return;
    };
    let session = source(&root, 1);
    let destination = root.join("out");
    let mut attempt =
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    attempt.advance(0, None).unwrap();
    std::os::unix::fs::symlink("missing", &destination).unwrap();
    assert!(attempt.publish().is_err());
    assert!(
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .is_err()
    );
    assert_eq!(
        std::fs::read_link(&destination).unwrap(),
        Path::new("missing")
    );
}

#[cfg(windows)]
#[path = "support/recovery_windows.rs"]
mod recovery_windows;

#[cfg(windows)]
#[test]
#[ignore = "requires native Windows NTFS, cmd.exe junction creation and symlink privilege/Developer Mode; run with --ignored"]
fn windows_reparse_destinations_are_occupied() {
    let Some(root) = recovery_windows::enter("windows_reparse_destinations_are_occupied") else {
        return;
    };
    let session = source(&root, 1);
    let external = root.join("external");
    std::fs::create_dir(&external).unwrap();
    std::fs::write(external.join("owned"), b"external").unwrap();
    for junction in [false, true] {
        let destination = root.join(if junction { "junction" } else { "dangling" });
        let mut attempt =
            records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
                .unwrap();
        attempt.advance(0, None).unwrap();
        recovery_windows::competitor(&destination, &external, junction);
        assert!(attempt.publish().is_err());
        assert!(
            records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
                .is_err()
        );
        recovery_windows::assert_reparse(&destination);
        assert_eq!(std::fs::read(external.join("owned")).unwrap(), b"external");
    }
}

#[test]
fn replaced_stage_directory_is_refused_with_file_identity_unchanged() {
    let Some(root) =
        isolated_profile::enter("replaced_stage_directory_is_refused_with_file_identity_unchanged")
    else {
        return;
    };
    let mut session = source(&root, 1);
    let nested = root.join("source/nested");
    std::fs::create_dir(&nested).unwrap();
    let file = nested.join("other.fasta");
    std::fs::write(&file, b">other\nACGT\n").unwrap();
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
    session.flush().unwrap();
    let destination = root.join("out");
    let mut attempt =
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let stage = attempt
        .check(0)
        .unwrap()
        .target()
        .parent()
        .unwrap()
        .to_path_buf();
    for document in 0..attempt.len() {
        attempt.advance(document, None).unwrap();
    }
    let before = object_identity(&stage.join("nested/other.fasta"), OrdinaryKind::File).unwrap();
    let retained = root.join("retained-stage-directory");
    std::fs::rename(stage.join("nested"), &retained).unwrap();
    std::fs::create_dir(stage.join("nested")).unwrap();
    std::fs::rename(
        retained.join("other.fasta"),
        stage.join("nested/other.fasta"),
    )
    .unwrap();
    assert_eq!(
        object_identity(&stage.join("nested/other.fasta"), OrdinaryKind::File).unwrap(),
        before
    );
    assert_ne!(
        object_identity(&retained, OrdinaryKind::Directory).unwrap(),
        object_identity(&stage.join("nested"), OrdinaryKind::Directory).unwrap()
    );
    assert!(attempt.publish().is_err());
    assert!(!destination.exists());
    assert!(
        attempt
            .status()
            .unwrap()
            .documents
            .iter()
            .all(|doc| doc.written)
    );
}
