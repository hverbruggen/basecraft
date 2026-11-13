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
    assert!(!session.can_undo(doc).unwrap());
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
    let doc = session.open(&path).unwrap();
    let replace = |start, residues: &str| Op::SeqReplace {
        start,
        end: start + 1,
        residues: residues.into(),
    };
    session.apply(doc, replace(0, "T")).unwrap();
    session.save(doc, None).unwrap();
    session.apply(doc, replace(1, "A")).unwrap();
    let recovered =
        crate::records::replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
    assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">sequence\nTAGT\n");
    assert_eq!(std::fs::read(&path).unwrap(), b">sequence\nTCGT\n");
    assert!(!source.join(".basecraft/recovery-references").exists());
}

#[test]
fn recovery_checkpoints_external_edits_and_undo_and_needs_replaced_originals() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::recovery_checkpoints_external_edits_and_undo_and_needs_replaced_originals",
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
                let block = [b'A'; 16 * 1024];
                let mut left = size;
                while left > 0 {
                    let count = left.min(block.len());
                    output.write_all(&block[..count]).unwrap();
                    left -= count;
                }
                output.write_all(b"\n").unwrap();
            }
            let mut session = Session::recoverable(&source).unwrap();
            let doc = session.open(&path).unwrap();
            let replace = |start, residue: &str| Op::SeqReplace {
                start,
                end: start + 1,
                residues: residue.into(),
            };
            session.apply(doc, replace(0, "T")).unwrap();
            let expected: &[u8] = match scenario {
                "replace" => {
                    let new = source.join("replacement");
                    std::fs::write(&new, b">outside\nCCCC\n").unwrap();
                    std::fs::rename(new, &path).unwrap();
                    b""
                }
                "save-edit-replace" => {
                    session.save(doc, None).unwrap();
                    session.apply(doc, replace(0, "G")).unwrap();
                    let new = source.join("replacement");
                    std::fs::write(&new, b">outside\nCCCC\n").unwrap();
                    std::fs::rename(new, &path).unwrap();
                    b""
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
            let outputs = crate::records::replay::recover(&recovery, &source.join("recovered"));
            if expected.is_empty() {
                // Replaced after the last edit: recovery needs the unchanged original.
                assert!(
                    outputs.map_or(true, |outputs| outputs.is_empty()),
                    "{scenario} {size}"
                );
                assert_ne!(crate::log::read_all(&recovery).unwrap().len(), 0);
                continue;
            }
            let outputs = outputs.unwrap();
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
fn cross_device_recovery_after_save_as_and_interrupted_save() {
    use std::os::unix::fs::MetadataExt;
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::cross_device_recovery_after_save_as_and_interrupted_save",
    ) else {
        return;
    };
    let disk = tempfile::tempdir_in("/dev/shm").unwrap();
    for scenario in [
        "before-rename",
        "after-rename",
        "save-as",
        "new-save-as",
        "changed-save-as",
    ] {
        let source = disk.path().join(scenario);
        std::fs::create_dir(&source).unwrap();
        let path = source.join("sequence.fa");
        std::fs::write(&path, b">sequence\nAAAA\n").unwrap();
        let mut session = Session::recoverable(&source).unwrap();
        assert_ne!(
            source.metadata().unwrap().dev(),
            root.metadata().unwrap().dev()
        );
        let doc = if scenario == "new-save-as" {
            session.create_sequence("sequence", "AAAA").unwrap()
        } else {
            session.open(&path).unwrap()
        };
        let edit = |start, text: &str| Op::SeqReplace {
            start,
            end: start + 1,
            residues: text.into(),
        };
        session.apply(doc, edit(0, "T")).unwrap();
        let expected = if scenario.ends_with("rename") {
            // Process loss before the data rename, or after it but before the
            // Save record: the pending intent stays for recovery.
            super::save_failure_tests::inject(if scenario == "before-rename" {
                super::save_failure_tests::WriteStep::Replace
            } else {
                super::save_failure_tests::WriteStep::Journal
            });
            assert!(session.save(doc, None).is_err());
            // The pending intent is published together with the data, so a
            // loss before the rename leaves only the journal to recover from.
            assert_eq!(
                session.recovery_root().join("pending-save.json").is_file(),
                scenario == "after-rename"
            );
            b"TAAA"
        } else {
            let saved = source.join(if scenario == "new-save-as" {
                "saved.gb"
            } else {
                "saved.fa"
            });
            session.save(doc, Some(&saved)).unwrap();
            session.apply(doc, edit(1, "G")).unwrap();
            std::fs::rename(&saved, source.join("unplugged.fa")).unwrap();
            if scenario == "changed-save-as" {
                session.apply(doc, edit(2, "C")).unwrap();
                assert!(session.recovery_warning(doc).is_some());
                b"TGCA"
            } else {
                b"TGAA"
            }
        };
        let recovery = session.recovery_root().to_path_buf();
        std::mem::forget(session);
        let output = root.join(scenario);
        let outputs = crate::records::replay::recover(&recovery, &output).unwrap();
        let restored = files::open(&outputs[0]).unwrap();
        assert_eq!(restored.doc.read(0, 4), expected, "{scenario}");
    }
}

#[test]
fn recovery_uses_an_immutable_object_after_the_original_changes() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::recovery_uses_an_immutable_object_after_the_original_changes",
    ) else {
        return;
    };
    let path = root.join("sequence.fa");
    let original = b">sequence\nAAAA\n";
    std::fs::write(&path, original).unwrap();
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&path).unwrap();
    let objects = objects_dir(session.recovery_root());
    std::fs::create_dir_all(&objects).unwrap();
    let object = objects.join(basecraft_io::hash_bytes(original).trim_start_matches("b3:"));
    std::fs::write(&object, original).unwrap();
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
    assert!(session.recovery_warning(doc).is_none());
    std::fs::write(&path, b">outside\nCCCC\n").unwrap();
    assert_eq!(std::fs::read(&object).unwrap(), original);
    let outputs =
        crate::records::replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
    assert_eq!(std::fs::read(&outputs[0]).unwrap(), b">sequence\nTAAA\n");
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
    let outputs =
        crate::records::replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
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
    let doc = session.open(&source).unwrap();
    let edit = |start, residues: &str| Op::SeqReplace {
        start,
        end: start + 1,
        residues: residues.into(),
    };
    session.apply(doc, edit(0, "G")).unwrap();
    std::fs::write(&source, b">outside\nCCCC\n").unwrap();
    session.apply(doc, edit(1, "T")).unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), b">outside\nCCCC\n");
    let recovered =
        crate::records::replay::recover(session.recovery_root(), &root.join("recovered")).unwrap();
    assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">outside\nGTAA\n");
}

#[cfg(target_os = "linux")]
#[test]
fn missing_cross_device_document_does_not_block_other_recovery() {
    let Some(root) = crate::isolated_profile::enter(
        "session::retention_tests::missing_cross_device_document_does_not_block_other_recovery",
    ) else {
        return;
    };
    let source = tempfile::tempdir_in("/dev/shm").unwrap();
    let mut session = Session::recoverable(source.path()).unwrap();
    for name in ["missing", "available"] {
        let path = source.path().join(format!("{name}.fa"));
        std::fs::write(&path, format!(">{name}\nAAAA\n")).unwrap();
        let doc = session.open(&path).unwrap();
        session
            .apply(
                doc,
                Op::SeqReplace {
                    start: 0,
                    end: 1,
                    residues: "G".into(),
                },
            )
            .unwrap();
    }
    std::fs::remove_file(source.path().join("missing.fa")).unwrap();
    let recovery = session.recovery_root().to_path_buf();
    std::mem::forget(session);
    let before = serde_json::to_vec(&crate::log::read_all(&recovery).unwrap()).unwrap();
    let outputs = crate::records::replay::recover(&recovery, &root.join("recovered")).unwrap();
    assert_eq!(outputs.len(), 1);
    assert_eq!(std::fs::read(&outputs[0]).unwrap(), b">available\nGAAA\n");
    assert_eq!(crate::log::read_all(&recovery).unwrap().len(), 4);
    assert_eq!(
        before,
        serde_json::to_vec(&crate::log::read_all(&recovery).unwrap()).unwrap()
    );
}
