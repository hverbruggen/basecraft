//! Retained recovery authority and publication, using isolated real files.
use basecraft_ops::{
    Error, Session,
    records::SavePolicy,
    records::{self, RecoveryMode},
};
use basecraft_platform::files::{OrdinaryKind, object_identity};
use basecraft_protocol::Op;
use std::path::Path;
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
    session
}
// Every analysis index below `root`, as text.
fn indexes(root: &Path) -> String {
    let mut text = String::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            text += &indexes(&path);
        } else if path
            .parent()
            .is_some_and(|p| p.ends_with(".basecraft/analysis-index"))
        {
            text += &std::fs::read_to_string(path).unwrap();
        }
    }
    text
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
fn receipts_are_single_use_session_document_and_phase_bound() {
    let Some(root) =
        isolated_profile::enter("receipts_are_single_use_session_document_and_phase_bound")
    else {
        return;
    };
    let session = source(&root, 2);
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

fn reference(anchor: &Path, source: &Path) -> basecraft_ops::records::ArtifactRun {
    std::fs::create_dir_all(anchor.parent().unwrap()).unwrap();
    basecraft_ops::records::reference_input_with_dependencies(
        anchor,
        source,
        &basecraft_io::hash_file(source).unwrap(),
        &[],
        "recovery test",
        None,
    )
    .unwrap()
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
    let indexes_before = indexes(&root);
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
            basecraft_ops::files::delimitation_companion(
                &work,
                &basecraft_io::hash_file(&path).map_err(std::io::Error::other)?,
                &path,
            )?
            .write(w)
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
    assert_eq!(indexes(&root), indexes_before);
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
    let registered = indexes(&root);
    let stage_name = stage.file_name().unwrap().to_str().unwrap();
    assert!(!registered.contains(stage_name), "{registered}");
    assert!(!stage.exists());
    for file in complete.outputs {
        let work = basecraft_ops::files::load_delimitation(&file).unwrap();
        assert_eq!(work.hypotheses.len(), 2);
        assert_eq!(work.exchange_fields["retained-unknown"], "keep");
        let side = basecraft_ops::files::tree_sidecar(&file);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(side).unwrap()).unwrap();
        assert_eq!(
            value["delimitation"]["path_base"],
            file.parent().unwrap().to_str().unwrap()
        );
        let run = basecraft_ops::records::ArtifactRun::for_result(&file)
            .unwrap()
            .unwrap();
        let input = basecraft_ops::records::read_input(&run).unwrap();
        let upstream = serde_json::to_value(&input.upstream).unwrap();
        let upstream = upstream.as_array().unwrap();
        assert!(upstream.iter().any(|record| {
            run.join(record["bundle"].as_str().unwrap())
                .canonicalize()
                .unwrap()
                == external.directory()
        }));
        // SPART inputs are kept under a SPART name.
        let retained = if file.extension().is_some_and(|ext| ext == "xml") {
            "submitted.spart.xml"
        } else {
            "submitted.spart"
        };
        assert_eq!(
            std::fs::read(run.join(retained)).unwrap(),
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
    let indexes_before = indexes(&root);
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
    assert_eq!(indexes(&root), indexes_before);
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

#[cfg(unix)]
#[test]
fn failed_recovery_write_spends_its_decision_and_publishes_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let Some(root) =
        isolated_profile::enter("failed_recovery_write_spends_its_decision_and_publishes_nothing")
    else {
        return;
    };
    let session = source(&root, 1);
    let destination = root.join("out");
    let mut attempt =
        records::prepare_recovery(session.recovery_root(), &destination, RecoveryMode::Strict)
            .unwrap();
    let check = attempt.check(0).unwrap();
    let stage = check.target().parent().unwrap().to_path_buf();
    // A full disk or a permission error stops the write into the stage.
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o500)).unwrap();
    let failed = attempt.advance(0, Some(check.clone().decide(SavePolicy::Cancel, false)));
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(failed, Err(Error::Format(_) | Error::Io(_))));
    assert!(!destination.exists());
    assert!(matches!(
        attempt.advance(0, Some(check.decide(SavePolicy::Cancel, false))),
        Err(Error::SaveDecisionStale(_))
    ));
}
