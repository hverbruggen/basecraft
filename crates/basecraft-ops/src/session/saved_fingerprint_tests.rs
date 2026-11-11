//! Current-payload reuse measured at the real serializer boundary, separately
//! from explicit oracle calls. No cache state is save or recovery authority.
use super::*;
use std::cell::Cell;

thread_local! { static ATTEMPTS: Cell<usize> = const { Cell::new(0) }; }
pub(super) fn attempt() {
    ATTEMPTS.with(|n| n.set(n.get() + 1));
}
fn count<T>(expected: usize, f: impl FnOnce() -> T) -> T {
    let before = ATTEMPTS.get();
    let result = f();
    assert_eq!(ATTEMPTS.get() - before, expected);
    result
}
fn oracle(s: &Session, id: DocId) -> Vec<String> {
    s.save_hashes_at(id, Path::new(&s.docs[id.0].key)).unwrap()
}
fn check(s: &Session, id: DocId, attempts: usize) -> Vec<String> {
    let hashes = count(attempts, || s.save_hashes(id).unwrap());
    assert_eq!(hashes, oracle(s, id));
    count(0, || assert_eq!(s.save_hashes(id).unwrap(), hashes));
    assert_eq!(
        s.document_record(id).unwrap().unsaved,
        hashes != s.docs[id.0].saved_state_hashes
    );
    hashes
}
fn fixture(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    std::fs::create_dir_all(root).unwrap();
    let path = root.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}
fn replace(residues: &str) -> Op {
    Op::SeqReplace {
        start: 0,
        end: 1,
        residues: residues.into(),
    }
}
fn sequence(root: &Path) -> (Session, DocId, PathBuf) {
    let path = fixture(root, "records.fa", b">a description\nACGT\n");
    let mut s = Session::new(root);
    let id = count(1, || s.open(&path).unwrap());
    (s, id, path)
}
fn invalid_format(s: &mut Session, id: DocId, format: files::Format) {
    let Loaded::Sequence(o) = s.docs[id.0].content_mut() else {
        panic!("sequence")
    };
    o.format = format;
}

#[test]
fn formats_queries_and_explicit_save_windows() {
    let Some(root) = crate::isolated_profile::enter(
        "session::saved_fingerprint_tests::formats_queries_and_explicit_save_windows",
    ) else {
        return;
    };
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Session>();
    let formats: &[(&str, &[u8], Op)] = &[
        ("records.fa", b">a description\nACGT\n>b retained\nTTAA\n", replace("T")),
        ("record.gb", b"LOCUS       a                          4 bp    DNA     linear   UNK 01-JAN-1970\nDEFINITION  example.\nACCESSION   a\nFEATURES             Location/Qualifiers\nORIGIN\n        1 acgt\n//\n", replace("T")),
        ("ragged.afa", b">a\nACGT\n>b\nAC\n", Op::AlnAddSequences { rows: vec![("c".into(), "TTTT".into())], reference: None }),
        ("forest.nwk", b"(a:1,b:2);\n(c:3,d:4);\n", Op::TreeRenameTip { from: "a".into(), to: "z".into() }),
        ("forest.nex", b"#NEXUS\nBEGIN TREES;\nTREE first = (a:1,b:2);\nTREE second = (c:3,d:4);\nEND;\n", Op::TreeRenameTip { from: "a".into(), to: "z".into() }),
    ];
    for (i, (name, bytes, op)) in formats.iter().enumerate() {
        let dir = root.join(i.to_string());
        let path = fixture(&dir, name, bytes);
        if i == 0 {
            std::fs::write(
                files::gff_sidecar(&path),
                b"##gff-version 3\na\t.\tgene\t1\t3\t.\t+\t.\tID=g\n",
            )
            .unwrap();
        }
        let mut s = Session::new(&dir);
        let id = count(1, || s.open(&path).unwrap());
        let baseline = oracle(&s, id);
        count(0, || {
            for _ in 0..3 {
                assert_eq!(s.save_hashes(id).unwrap(), baseline);
                assert!(!s.document_record(id).unwrap().unsaved);
                assert!(s.saved_source(id).unwrap().is_some());
            }
            match &s.docs[id.0].content {
                Loaded::Sequence(o) => {
                    o.doc.index();
                    o.doc.summary();
                }
                Loaded::Alignment(a) => {
                    a.doc.consensus(0, a.doc.width());
                    a.doc.column_stats_range(0, a.doc.width());
                }
                _ => {}
            }
            assert_eq!(s.save_hashes(id).unwrap(), baseline);
            s.save(id, None).unwrap();
        });
        assert!(!crate::log::log_dir(&dir).exists());
        count(2, || {
            assert_eq!(oracle(&s, id), baseline);
            assert_eq!(oracle(&s, id), baseline);
        });
        s.apply(id, op.clone()).unwrap();
        let changed = check(&s, id, 1);
        assert_ne!(changed, baseline);
        assert!(s.saved_source(id).unwrap().is_none());
        count(0, || s.save(id, None).unwrap());
        assert_eq!(s.docs[id.0].saved_state_hashes, changed);
        check(&s, id, 0);
        s.undo(id).unwrap();
        check(&s, id, 1);
        s.redo(id).unwrap();
        assert_eq!(check(&s, id, 1), changed);
    }
}

#[test]
fn equivalent_edits_features_merges_groups_and_branching() {
    let Some(root) = crate::isolated_profile::enter(
        "session::saved_fingerprint_tests::equivalent_edits_features_merges_groups_and_branching",
    ) else {
        return;
    };
    let (mut s, id, _) = sequence(&root);
    let baseline = oracle(&s, id);
    for text in ["A", "T", "A"] {
        s.apply(id, replace(text)).unwrap();
        let hashes = check(&s, id, 1);
        assert_eq!(hashes == baseline, text == "A");
    }
    assert_eq!(s.docs[id.0].undo.len(), 3);
    let feature = Feature {
        kind: "gene".into(),
        location: basecraft_protocol::Location::simple(0, 2, basecraft_protocol::Strand::Forward),
        qualifiers: vec![],
    };
    let fid = s
        .apply(
            id,
            Op::FeatureAdd {
                feature: feature.clone(),
            },
        )
        .unwrap()
        .feature
        .unwrap();
    let annotated = check(&s, id, 1);
    s.apply(id, Op::FeatureUpdate { id: fid.0, feature })
        .unwrap();
    assert_eq!(check(&s, id, 1), annotated);
    s.undo_single(id).unwrap();
    check(&s, id, 1);
    s.undo_single(id).unwrap();
    assert_eq!(check(&s, id, 1), baseline);
    s.redo_single(id).unwrap();
    assert_eq!(check(&s, id, 1), annotated);
    assert!(s.doc(id).unwrap().feature(fid).is_some());
    let first = s
        .apply(
            id,
            Op::SeqReplace {
                start: 4,
                end: 4,
                residues: "A".into(),
            },
        )
        .unwrap();
    check(&s, id, 1);
    let second = s
        .apply(
            id,
            Op::SeqReplace {
                start: 5,
                end: 5,
                residues: "C".into(),
            },
        )
        .unwrap();
    check(&s, id, 1);
    assert_eq!(first.entry_id, second.entry_id);
    s.apply(
        id,
        Op::SeqReplace {
            start: 0,
            end: 0,
            residues: "T".into(),
        },
    )
    .unwrap();
    check(&s, id, 1);
    assert_ne!(s.docs[id.0].undo.last().unwrap().entry_id, first.entry_id);
    s.flush().unwrap();
    check(&s, id, 0);
    let group = s.begin_group();
    s.apply(id, replace("G")).unwrap();
    check(&s, id, 1);
    s.end_group();
    s.resume_group(id, group).unwrap();
    s.apply(id, replace("C")).unwrap();
    check(&s, id, 1);
    s.end_group();
    s.undo(id).unwrap();
    check(&s, id, 1);
    s.redo(id).unwrap();
    check(&s, id, 1);
    s.undo(id).unwrap();
    check(&s, id, 1);
    s.apply(id, replace("A")).unwrap();
    check(&s, id, 1);
    assert!(!s.can_redo(id));
}

#[test]
fn live_journal_rollback_and_staged_proposals() {
    let Some(root) = crate::isolated_profile::enter(
        "session::saved_fingerprint_tests::live_journal_rollback_and_staged_proposals",
    ) else {
        return;
    };
    let (mut s, id, _) = sequence(&root);
    s.track(id).unwrap();
    let baseline = check(&s, id, 0);
    s.log.as_mut().unwrap().faults.sync = true;
    assert!(s.apply(id, replace("T")).is_err());
    assert_eq!(s.generation(id), 0);
    assert_eq!(check(&s, id, 1), baseline);
    let feature = Feature {
        kind: "gene".into(),
        location: basecraft_protocol::Location::simple(0, 2, basecraft_protocol::Strand::Forward),
        qualifiers: vec![],
    };
    let fid = s
        .apply(id, Op::FeatureAdd { feature })
        .unwrap()
        .feature
        .unwrap();
    let edited = check(&s, id, 1);
    let generation = s.generation(id);
    s.log.as_mut().unwrap().faults.sync = true;
    assert!(s.undo(id).is_err());
    assert_eq!(s.generation(id), generation);
    assert_eq!(check(&s, id, 1), edited);
    assert!(s.doc(id).unwrap().feature(fid).is_some());
    s.undo(id).unwrap();
    assert_eq!(check(&s, id, 1), baseline);
    let generation = s.generation(id);
    s.log.as_mut().unwrap().faults.sync = true;
    assert!(s.redo(id).is_err());
    assert_eq!(s.generation(id), generation);
    assert_eq!(check(&s, id, 1), baseline);
    let actor = Actor::Agent {
        model: "fingerprint-test".into(),
    };
    let revision = s.revision(id);
    assert!(
        s.apply_sequence_proposals(
            id,
            generation,
            revision,
            &[Op::SeqReplace {
                start: 99,
                end: 100,
                residues: "T".into()
            }],
            &actor
        )
        .is_err()
    );
    check(&s, id, 0);
    s.log.as_mut().unwrap().faults.after_bytes = Some(127);
    assert!(
        s.apply_sequence_proposals(id, generation, revision, &[replace("T")], &actor)
            .is_err()
    );
    check(&s, id, 0);
    s.apply_sequence_proposals(
        id,
        generation,
        revision,
        &[replace("T"), replace("G")],
        &actor,
    )
    .unwrap();
    check(&s, id, 1);
    s.undo(id).unwrap();
    assert_eq!(check(&s, id, 1), baseline);
    s.redo(id).unwrap();
    check(&s, id, 1);
}

#[test]
fn errors_are_retried_and_creation_is_not_saved() {
    let Some(root) = crate::isolated_profile::enter(
        "session::saved_fingerprint_tests::errors_are_retried_and_creation_is_not_saved",
    ) else {
        return;
    };
    let (mut s, id, _) = sequence(&root);
    let baseline = oracle(&s, id);
    invalid_format(&mut s, id, files::Format::Metadata);
    let errors = count(2, || {
        (
            s.save_hashes(id).unwrap_err(),
            s.save_hashes(id).unwrap_err(),
        )
    });
    assert_eq!(errors.0.to_string(), errors.1.to_string());
    assert!(s.docs[id.0].current_saved_hashes.get().is_none());
    assert_eq!(
        errors.0.to_string(),
        s.save_hashes_at(id, Path::new("records.fa"))
            .unwrap_err()
            .to_string()
    );
    invalid_format(&mut s, id, files::Format::Fasta);
    assert_eq!(check(&s, id, 1), baseline);
    let new = s.create_sequence("new", "ACGT").unwrap();
    assert!(s.docs[new.0].saved_state_hashes.is_empty());
    check(&s, new, 1);
    assert!(s.document_record(new).unwrap().unsaved);
    assert!(s.saved_source(new).unwrap().is_none());
}

#[test]
fn tree_payload_and_excluded_kinds() {
    let Some(root) = crate::isolated_profile::enter(
        "session::saved_fingerprint_tests::tree_payload_and_excluded_kinds",
    ) else {
        return;
    };
    let path = fixture(&root, "tree.nwk", b"((a:1,b:1):2,c:3);\n(d:1,e:2);\n");
    let mut s = Session::new(&root);
    let id = s.open(&path).unwrap();
    let baseline = check(&s, id, 0);
    let style = s.tree(id).unwrap().style().clone();
    s.apply(
        id,
        Op::TreeStyle {
            style,
            remove_supports: vec![],
        },
    )
    .unwrap();
    assert_eq!(check(&s, id, 1), baseline);
    s.apply(
        id,
        Op::TreeAddSupport {
            name: "support".into(),
            source: "test".into(),
            values: vec![Some("90".into()), Some("80".into())],
            kind: Default::default(),
        },
    )
    .unwrap();
    let support = check(&s, id, 1);
    assert_ne!(support, baseline);
    s.apply(
        id,
        Op::TreeAnnotate {
            id: 1,
            annotation: Some(basecraft_protocol::CladeAnnotation::new(
                vec!["a".into()],
                "note",
            )),
        },
    )
    .unwrap();
    assert_ne!(check(&s, id, 1), support);
    s.undo(id).unwrap();
    assert_eq!(check(&s, id, 1), support);
    s.undo(id).unwrap();
    assert_eq!(check(&s, id, 1), baseline);
    let metadata = crate::metadata::create(&root).unwrap();
    let alignment = fixture(&root, "source.afa", b">a\nACGT\n>b\nACGT\n");
    let submission = root.join("review.submission.json");
    crate::submission::create(&mut Session::unlogged(&root), &submission, &alignment).unwrap();
    let spart = fixture(
        &root,
        "hypotheses.spart",
        include_bytes!(
            "../../../../testdata/fixtures/compact/species-delimitation/hypotheses.spart"
        ),
    );
    for path in [&metadata, &submission, &spart] {
        let id = count(1, || s.open(path).unwrap());
        let expected = oracle(&s, id);
        count(2, || {
            assert_eq!(s.save_hashes(id).unwrap(), expected);
            assert_eq!(s.save_hashes(id).unwrap(), expected);
        });
        assert!(s.docs[id.0].current_saved_hashes.get().is_none());
    }
}

#[cfg(unix)]
#[test]
#[cfg(unix)]
fn delimitation_tracks_external_parent_and_placement() {
    let Some(root) = crate::isolated_profile::enter(
        "session::saved_fingerprint_tests::delimitation_tracks_external_parent_and_placement",
    ) else {
        return;
    };
    let bytes = include_bytes!(
        "../../../../testdata/fixtures/compact/species-delimitation/hypotheses.spart"
    );
    let a = root.join("a");
    let b = root.join("b");
    fixture(&a, "hypotheses.spart", bytes);
    fixture(&b, "hypotheses.spart", bytes);
    let link = root.join("link");
    std::os::unix::fs::symlink(&a, &link).unwrap();
    let mut s = Session::new(&root);
    let id = s.open(&link.join("hypotheses.spart")).unwrap();
    // The current key can be canonicalized on open. Select the public target
    // spelling for this private negative control, without touching the payload.
    s.docs[id.0].key = "link/hypotheses.spart".into();
    let before = count(1, || s.save_hashes(id).unwrap());
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&b, &link).unwrap();
    let after = count(1, || s.save_hashes(id).unwrap());
    assert_eq!(after, oracle(&s, id));
    assert_ne!(before, after);
    let xml = s.save_hashes_at(id, &root.join("other.spart.xml")).unwrap();
    assert_ne!(xml, after);
    s.write_publication = crate::documents::WritePublication::Recovery {
        stage: root.clone(),
        destination: root.join("recovered"),
        stores: Mutex::new(vec![]),
    };
    let placed = count(1, || s.save_hashes(id).unwrap());
    assert_eq!(placed, oracle(&s, id));
    assert_ne!(placed, after);
    assert!(s.docs[id.0].current_saved_hashes.get().is_none());
}

#[test]
fn aliases_exports_and_changed_sources_keep_authority() {
    let Some(root) = crate::isolated_profile::enter(
        "session::saved_fingerprint_tests::aliases_exports_and_changed_sources_keep_authority",
    ) else {
        return;
    };
    for mode in 0..4 {
        let dir = root.join(mode.to_string());
        let path = fixture(&dir, "source.fa", b">a\nACGT\n");
        let mut s = match mode {
            0 => Session::new(&dir),
            1 => Session::unlogged(&dir),
            2 => Session::detached(&dir),
            _ => Session::recoverable(&dir).unwrap(),
        };
        let id = s.open(&path).unwrap();
        let baseline = check(&s, id, 0);
        s.apply(id, replace("T")).unwrap();
        let changed = check(&s, id, 1);
        s.save(id, Some(&dir.join("export.fa"))).unwrap();
        assert_eq!(s.docs[id.0].saved_state_hashes, baseline);
        assert_eq!(check(&s, id, 0), changed);
        s.save(id, Some(&dir.join("./source.fa"))).unwrap();
        assert_eq!(s.docs[id.0].saved_state_hashes, changed);
        check(&s, id, 0);
    }
    for save in [false, true] {
        let dir = root.join(format!("changed-{save}"));
        let (mut s, id, path) = sequence(&dir);
        let baseline = check(&s, id, 0);
        let stamp = path.metadata().unwrap().modified().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[15] = b'T';
        std::fs::write(&path, bytes).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(stamp)
            .unwrap();
        if save {
            assert!(s.save_history(id).is_err());
        } else {
            assert!(s.apply(id, replace("T")).is_err());
        }
        assert_eq!(check(&s, id, 0), baseline);
    }
}

#[test]
fn drop_keeps_flush_pending_dirty_and_error_gates() {
    let Some(root) = crate::isolated_profile::enter(
        "session::saved_fingerprint_tests::drop_keeps_flush_pending_dirty_and_error_gates",
    ) else {
        return;
    };
    for mode in 0..6 {
        let dir = root.join(mode.to_string());
        let path = fixture(&dir, "source.fa", b">a\nACGT\n");
        let mut s = Session::recoverable(&dir).unwrap();
        let id = s.open(&path).unwrap();
        let recovery = s.recovery_root().to_owned();
        assert!(!recovery.exists());
        if mode == 0 {
            count(0, || drop(s));
            assert!(!recovery.exists());
            continue;
        }
        s.apply(id, replace("A")).unwrap();
        check(&s, id, 1);
        assert!(recovery.exists());
        match mode {
            2 => {
                let second = fixture(&dir, "second.fa", b">b\nACGT\n");
                let other = s.open(&second).unwrap();
                s.apply(other, replace("T")).unwrap();
                check(&s, other, 1);
            }
            3 => {
                std::fs::write(recovery.join("pending-save.json"), b"pending").unwrap();
            }
            4 => {
                s.merge = true;
                s.apply(
                    id,
                    Op::SeqReplace {
                        start: 0,
                        end: 0,
                        residues: "A".into(),
                    },
                )
                .unwrap();
                check(&s, id, 1);
                s.log.as_mut().unwrap().faults.sync = true;
            }
            5 => {
                invalid_format(&mut s, id, files::Format::Metadata);
            }
            _ => {}
        }
        count(usize::from(mode == 5), || drop(s));
        assert_eq!(recovery.exists(), mode != 1);
    }
}
