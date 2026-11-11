use super::*;

#[test]
fn changed_original_is_rejected_before_editing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sequence.fa");
    std::fs::write(&path, b">sequence\nACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    std::fs::write(&path, b">sequence\nTTTT\n").unwrap();
    assert!(
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 0,
                    end: 1,
                    residues: "G".into()
                }
            )
            .is_err()
    );
    assert!(!session.can_undo(doc));
    assert!(!session.document_record(doc).unwrap().unsaved);
}

#[test]
fn recovery_starts_at_completed_save() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::recovery_starts_at_completed_save",
    ) else {
        return;
    };
    let source = root.join("source");
    std::fs::create_dir(&source).unwrap();
    let path = source.join("sequence.fa");
    std::fs::write(&path, b">sequence\nACGT\n").unwrap();
    let mut session = Session::new(&source);
    session.provenance_policy = basecraft_platform::settings::ProvenancePolicy::None;
    session.merge = false;
    let doc = session.open(&path).unwrap();
    let replace = |start, residues: &str| Op::SeqReplace {
        start,
        end: start + 1,
        residues: residues.into(),
    };
    session.apply(doc, replace(0, "T")).unwrap();
    session.save(doc, None).unwrap();
    session.apply(doc, replace(1, "A")).unwrap();
    session.flush().unwrap();
    let recovered =
        crate::replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
    assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">sequence\nTAGT\n");
    assert_eq!(std::fs::read(&path).unwrap(), b">sequence\nTCGT\n");
    assert!(!source.join(".basecraft/recovery-references").exists());
}

#[test]
fn recovery_retains_replaced_sources_and_checkpoints_external_edits_and_undo() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::recovery_retains_replaced_sources_and_checkpoints_external_edits_and_undo",
    ) else {
        return;
    };
    for size in [4, 101 * 1024 * 1024] {
        for scenario in [
            "replace",
            "rewrite-edit",
            "save-undo",
            "save-undo-redo",
            "save-edit-replace",
        ] {
            let source = root.join(format!("{size}-{scenario}"));
            std::fs::create_dir(&source).unwrap();
            let path = source.join("sequence.fa");
            {
                use std::io::Write;
                let mut output = std::fs::File::create(&path).unwrap();
                output.write_all(b">sequence\n").unwrap();
                let block = [b'A'; 64 * 1024];
                let mut left = size;
                while left > 0 {
                    let count = left.min(block.len());
                    output.write_all(&block[..count]).unwrap();
                    left -= count;
                }
                output.write_all(b"\n").unwrap();
            }
            let mut session = Session::recoverable(&source).unwrap();
            session.provenance_policy = basecraft_platform::settings::ProvenancePolicy::None;
            let doc = session.open(&path).unwrap();
            let replace = |start, residue: &str| Op::SeqReplace {
                start,
                end: start + 1,
                residues: residue.into(),
            };
            session.apply(doc, replace(0, "T")).unwrap();
            let retained = objects_dir(session.recovery_root()).join(
                session.docs[doc.0].accepted_hashes[0]
                    .1
                    .trim_start_matches("b3:"),
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert_eq!(
                    path.metadata().unwrap().ino(),
                    retained.metadata().unwrap().ino()
                );
            }
            let expected = match scenario {
                "replace" => {
                    let new = source.join("replacement");
                    std::fs::write(&new, b">outside\nCCCC\n").unwrap();
                    std::fs::rename(new, &path).unwrap();
                    b"TAAA"
                }
                "save-edit-replace" => {
                    session.save(doc, None).unwrap();
                    session.apply(doc, replace(0, "G")).unwrap();
                    let new = source.join("replacement");
                    std::fs::write(&new, b">outside\nCCCC\n").unwrap();
                    std::fs::rename(new, &path).unwrap();
                    b"GAAA"
                }
                "rewrite-edit" => {
                    std::fs::write(&path, b">outside\nCCCC\n").unwrap();
                    session.apply(doc, replace(1, "G")).unwrap();
                    assert!(
                        session
                            .document_record(doc)
                            .unwrap()
                            .warnings
                            .iter()
                            .any(|warning| warning.contains("changed on disk"))
                    );
                    b"TGAA"
                }
                _ => {
                    session.save(doc, None).unwrap();
                    session.undo(doc).unwrap();
                    if scenario == "save-undo-redo" {
                        session.redo(doc).unwrap();
                        b"TAAA"
                    } else {
                        b"AAAA"
                    }
                }
            };
            let recovery = session.recovery_root().to_path_buf();
            std::mem::forget(session);
            let outputs = crate::replay::recover(&recovery, &source.join("recovered")).unwrap();
            let mut check = Session::unlogged(outputs[0].parent().unwrap());
            let doc = check.open(&outputs[0]).unwrap();
            assert_eq!(
                check.doc(doc).unwrap().read(0, 4),
                expected,
                "{scenario} {size}"
            );
            assert_eq!(check.doc(doc).unwrap().len(), size as u64);
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn different_filesystems_keep_references_and_pending_save_recovers_after_rename() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::different_filesystems_keep_references_and_pending_save_recovers_after_rename",
    ) else {
        return;
    };
    let state = tempfile::tempdir_in("/dev/shm").unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_ne!(
        root.metadata().unwrap().dev(),
        state.path().metadata().unwrap().dev()
    );
    for scenario in ["replace", "before-rename", "after-rename"] {
        let source = root.join(scenario);
        std::fs::create_dir(&source).unwrap();
        let path = source.join("sequence.fa");
        std::fs::write(&path, b">sequence\nAAAA\n").unwrap();
        let mut session = Session::new(&source);
        session.storage_root = state.path().join(scenario);
        session.log = Some(crate::log::LogWriter::new(&session.storage_root, "test"));
        session.provenance_policy = basecraft_platform::settings::ProvenancePolicy::None;
        session.merge = false;
        let doc = session.open(&path).unwrap();
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 0,
                    end: 1,
                    residues: "T".into(),
                },
            )
            .unwrap();
        assert_eq!(
            std::fs::read_dir(objects_dir(session.recovery_root()))
                .unwrap()
                .count(),
            0
        );
        if scenario == "replace" {
            std::fs::write(&path, b">outside\nCCCC\n").unwrap();
            assert!(
                crate::replay::recover(session.recovery_root(), &source.join("recovered"))
                    .unwrap_err()
                    .to_string()
                    .contains("unchanged original")
            );
            assert_eq!(
                crate::log::read_all(session.recovery_root()).unwrap().len(),
                2
            );
            continue;
        }
        let target = source.join("prepared/sequence.fa");
        std::fs::create_dir(target.parent().unwrap()).unwrap();
        let written = session.docs[doc.0]
            .content
            .as_document()
            .write(
                &target,
                files::Format::Fasta,
                session.recovery_root(),
                &crate::documents::WritePublication::Immediate,
            )
            .unwrap();
        let hashes: Vec<_> = written
            .iter()
            .map(|(path, hash)| {
                (
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    hash.clone(),
                )
            })
            .collect();
        let baseline = session.entry(
            doc,
            session.actor.clone(),
            Op::Save {
                path: "sequence.fa".into(),
            },
            OpResult {
                hashes: hashes.clone(),
                feature: None,
            },
        );
        let pending = basecraft_protocol::records::PendingSave {
            baseline,
            schema_version: 1,
            session: session.id.clone(),
            target: path.clone(),
            before_hash: Some(basecraft_io::hash_file(&path).unwrap()),
            expected_hashes: hashes.iter().map(|(_, hash)| hash.clone()).collect(),
            status: basecraft_protocol::records::RecoveryStatus::Pending,
            note: String::new(),
            extra: Default::default(),
        };
        std::fs::write(
            session.recovery_root().join("pending-save.json"),
            serde_json::to_vec(&pending).unwrap(),
        )
        .unwrap();
        if scenario == "after-rename" {
            for (file, _) in written {
                std::fs::rename(&file, source.join(file.file_name().unwrap())).unwrap();
            }
        }
        let recovered =
            crate::replay::recover(session.recovery_root(), &source.join("recovered")).unwrap();
        assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">sequence\nTAAA\n");
    }
}

#[test]
fn recovery_after_save_keeps_feature_ids_and_external_save_as() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::recovery_after_save_keeps_feature_ids_and_external_save_as",
    ) else {
        return;
    };
    let source = root.join("source");
    let other = root.join("other");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&other).unwrap();
    let file = source.join("sequence.gb");
    std::fs::write(
        &file,
        include_bytes!("../../../../testdata/fixtures/compact/sequences-annotations/locus.gb"),
    )
    .unwrap();
    let mut session = Session::new(&source);
    session.merge = false;
    session.provenance_policy = basecraft_platform::settings::ProvenancePolicy::None;
    let doc = session.open(&file).unwrap();
    let feature = session
        .doc(doc)
        .unwrap()
        .features()
        .next()
        .unwrap()
        .1
        .clone();
    session.apply(doc, Op::FeatureRemove { id: 0 }).unwrap();
    let saved = other.join("copy.gb");
    session.save(doc, Some(&saved)).unwrap();
    let added = session
        .apply(doc, Op::FeatureAdd { feature })
        .unwrap()
        .feature
        .unwrap();
    session
        .apply(doc, Op::FeatureRemove { id: added.0 })
        .unwrap();
    let expected = session.doc(doc).unwrap().to_genbank();
    let outputs = crate::replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
    let restored = files::open(&outputs[0]).unwrap();
    assert_eq!(restored.doc.to_genbank(), expected);
}

#[test]
fn external_document_checkpoint_stays_inside_recovery_storage() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::external_document_checkpoint_stays_inside_recovery_storage",
    ) else {
        return;
    };
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    let source = root.join("outside.fa");
    std::fs::write(&source, b">outside\nAAAA\n").unwrap();
    let mut session = Session::recoverable(&project).unwrap();
    session.provenance_policy = basecraft_platform::settings::ProvenancePolicy::None;
    let doc = session.open(&source).unwrap();
    let edit = |start, residues: &str| Op::SeqReplace {
        start,
        end: start + 1,
        residues: residues.into(),
    };
    session.apply(doc, edit(0, "G")).unwrap();
    std::fs::write(&source, b">outside\nCCCC\n").unwrap();
    session.apply(doc, edit(1, "T")).unwrap();
    session.flush().unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), b">outside\nCCCC\n");
    let recovered =
        crate::replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
    assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">outside\nGTAA\n");
}
