//! Ordinary saves must not turn ragged input into an asserted alignment.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::Session;
use basecraft_protocol::Op;

const RAGGED: &str = ">a long description\nACGT--\n>b short description\nA.-\n";

#[test]
fn ragged_save_and_save_as_reopen_preserve_explicit_lengths_and_gaps() {
    crate::isolated_profile::isolate!();
    for save_as in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.afa");
        let destination = dir.path().join("copy.afa");
        std::fs::write(&source, RAGGED).unwrap();
        let target = if save_as { &destination } else { &source };
        {
            let mut session = Session::new(dir.path());
            let doc = session.open(&source).unwrap();
            assert!(
                session
                    .alignment(doc)
                    .unwrap()
                    .validate_aligned_input()
                    .is_err()
            );
            session
                .apply(
                    doc,
                    Op::AlnReplace {
                        row: 1,
                        start: 0,
                        end: 1,
                        residues: "T".into(),
                    },
                )
                .unwrap();
            session
                .save(doc, save_as.then_some(target.as_path()))
                .unwrap();
            assert!(
                session
                    .alignment(doc)
                    .unwrap()
                    .validate_aligned_input()
                    .is_err()
            );
            // Repeat the same save route: its accepted hash and serializer
            // must agree, without introducing display padding.
            session
                .save(doc, save_as.then_some(target.as_path()))
                .unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            RAGGED.replace("A.-", "T.-")
        );
        if save_as {
            assert_eq!(std::fs::read_to_string(&source).unwrap(), RAGGED);
        }
        let mut reopened = Session::new(dir.path());
        let doc = reopened.open(target).unwrap();
        let alignment = reopened.alignment(doc).unwrap();
        assert!(alignment.validate_aligned_input().is_err());
        assert_eq!(alignment.saved_row_len(1), 3);
        assert_eq!(alignment.read(1, 0, 6), b"T.----");
    }
}

#[test]
fn ragged_extent_tracks_explicit_gap_edits_undo_redo_and_snapshots() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.afa");
    std::fs::write(&source, RAGGED).unwrap();
    {
        let mut session = Session::new(dir.path());
        let doc = session.open(&source).unwrap();
        // Editing a synthetic cell makes that extent explicit, even for '-'.
        session
            .apply(
                doc,
                Op::AlnReplace {
                    row: 1,
                    start: 4,
                    end: 5,
                    residues: "-".into(),
                },
            )
            .unwrap();
        assert_eq!(session.alignment(doc).unwrap().saved_row_len(1), 5);
        assert!(
            session
                .alignment(doc)
                .unwrap()
                .validate_aligned_input()
                .is_err()
        );
        session.undo(doc).unwrap();
        assert_eq!(session.alignment(doc).unwrap().saved_row_len(1), 3);
        session.redo(doc).unwrap();
        assert_eq!(
            session.alignment(doc).unwrap().snapshot().saved_row_len(1),
            5
        );
        session.undo(doc).unwrap();
        // Explicitly supplied terminal gaps are meaningful input. Equal
        // lengths permit the input; no biological alignment is inferred.
        session
            .apply(
                doc,
                Op::AlnReplace {
                    row: 1,
                    start: 3,
                    end: 6,
                    residues: "---".into(),
                },
            )
            .unwrap();
        assert!(
            session
                .alignment(doc)
                .unwrap()
                .validate_aligned_input()
                .is_ok()
        );
        session.undo(doc).unwrap();
        assert!(
            session
                .alignment(doc)
                .unwrap()
                .validate_aligned_input()
                .is_err()
        );
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        RAGGED.replace("A.-\n", "A.----\n")
    );
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    assert!(
        session
            .alignment(doc)
            .unwrap()
            .validate_aligned_input()
            .is_ok()
    );
    assert_eq!(session.alignment(doc).unwrap().read(1, 0, 6), b"A.----");
}

#[test]
fn ragged_row_extents_follow_reordering_and_undo_after_width_growth() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.afa");
    std::fs::write(&source, RAGGED).unwrap();
    {
        let mut session = Session::new(dir.path());
        let doc = session.open(&source).unwrap();
        session
            .apply(
                doc,
                Op::AlnReplace {
                    row: 0,
                    start: 0,
                    end: 0,
                    residues: "N".into(),
                },
            )
            .unwrap();
        assert_eq!(session.alignment(doc).unwrap().width(), 7);
        assert_eq!(session.alignment(doc).unwrap().saved_row_len(1), 3);
        session
            .apply(
                doc,
                Op::AlnMoveRows {
                    rows: vec![(0, 1)],
                    to: 1,
                },
            )
            .unwrap();
        assert_eq!(session.alignment(doc).unwrap().saved_row_len(0), 3);
        session.undo(doc).unwrap();
        assert_eq!(session.alignment(doc).unwrap().saved_row_len(1), 3);
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        ">b short description\nA.-\n>a long description\nNACGT--\n"
    );
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    assert!(
        session
            .alignment(doc)
            .unwrap()
            .validate_aligned_input()
            .is_err()
    );
}

#[test]
fn equal_alignment_preserves_literal_terminal_gaps_and_existing_delete_semantics() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.afa");
    std::fs::write(&source, ">a description\nAC--..\n>b\nA-CG--\n").unwrap();
    {
        let mut session = Session::new(dir.path());
        let doc = session.open(&source).unwrap();
        session
            .apply(
                doc,
                Op::AlnDelete {
                    rows: vec![(0, 1)],
                    start: 0,
                    end: 1,
                },
            )
            .unwrap();
        assert_eq!(session.alignment(doc).unwrap().read(0, 0, 6), b"C--..-");
        assert!(
            session
                .alignment(doc)
                .unwrap()
                .validate_aligned_input()
                .is_ok()
        );
        session.undo(doc).unwrap();
        assert_eq!(session.alignment(doc).unwrap().read(0, 0, 6), b"AC--..");
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        ">a description\nC--..-\n>b\nA-CG--\n"
    );
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    assert!(
        session
            .alignment(doc)
            .unwrap()
            .validate_aligned_input()
            .is_ok()
    );
}

#[test]
fn alignment_interface_rejections_normalization_and_hashes() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.afa");
    std::fs::write(&source, RAGGED).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    let original_hashes = session.save_hashes(doc).unwrap();
    let original_revision = session.revision(doc).unwrap();
    for op in [
        Op::SeqReplace {
            start: 0,
            end: 1,
            residues: "T".into(),
        },
        Op::AlnReplace {
            row: 99,
            start: 0,
            end: 1,
            residues: "T".into(),
        },
        Op::AlnReplace {
            row: 0,
            start: 0,
            end: 1,
            residues: "?".into(),
        },
        Op::AlnFill {
            rows: vec![(0, 1)],
            start: 0,
            end: 1,
            residue: "AC".into(),
        },
    ] {
        // Operations of another document kind are unsupported, not invalid.
        let wrong_kind = matches!(op, Op::SeqReplace { .. });
        let result = session.apply(doc, op);
        assert!(
            matches!(result, Err(basecraft_ops::Error::Unsupported(_))) == wrong_kind
                && matches!(result, Err(basecraft_ops::Error::Invalid(_))) != wrong_kind,
            "{result:?}"
        );
        assert_eq!(session.revision(doc).unwrap(), original_revision);
        assert_eq!(session.save_hashes(doc).unwrap(), original_hashes);
        assert!(!session.can_undo(doc).unwrap());
    }
    session
        .apply(
            doc,
            Op::AlnReplace {
                row: 1,
                start: 0,
                end: 1,
                residues: "t".into(),
            },
        )
        .unwrap();
    assert_eq!(session.alignment(doc).unwrap().read(1, 0, 3), b"T.-");
    let expected = RAGGED.replace("A.-", "T.-");
    let hashes = vec![basecraft_io::hash_bytes(expected.as_bytes())];
    assert_eq!(session.save_hashes(doc).unwrap(), hashes);
    let revision = session.revision(doc).unwrap();
    let wrong = dir.path().join("wrong.gb");
    assert!(matches!(
        session.save(doc, Some(&wrong)),
        Err(basecraft_ops::Error::Invalid(_))
    ));
    assert!(!wrong.exists());
    assert_eq!(session.revision(doc).unwrap(), revision);
    // Historical saved hashes ignore the target even when writing rejects it.
    assert_eq!(session.save_hashes_at(doc, &wrong).unwrap(), hashes);
    session.undo(doc).unwrap();
    assert_eq!(session.save_hashes(doc).unwrap(), original_hashes);
    session.redo(doc).unwrap();
    session.save(doc, None).unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), expected.as_bytes());
    drop(session);
    let report = basecraft_ops::records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}

#[test]
fn alignment_bursts_respect_actor_group_and_nonadjacent_boundaries() {
    use basecraft_protocol::Actor;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.afa");
    std::fs::write(&source, ">a\nAAAAAAAA\n>b\nAAAAAAAA\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    let replace = |start, residue: &str| Op::AlnReplace {
        row: 0,
        start,
        end: start + 1,
        residues: residue.into(),
    };
    let first = Actor::Script {
        client: "first".into(),
    };
    let second = Actor::Script {
        client: "second".into(),
    };
    session.set_actor(first.clone());
    session.apply(doc, replace(0, "C")).unwrap();
    session.apply(doc, replace(1, "G")).unwrap();
    session.set_actor(second.clone());
    session.apply(doc, replace(2, "T")).unwrap();
    // Group boundaries retain the existing caller-owned flush contract.
    session.flush().unwrap();
    session.begin_group();
    session.apply(doc, replace(3, "C")).unwrap();
    session.apply(doc, replace(4, "G")).unwrap();
    session.flush().unwrap();
    session.end_group();
    session.apply(doc, replace(5, "T")).unwrap();
    session.apply(doc, replace(7, "C")).unwrap();
    for expected in [
        b"CGTCGTAA",
        b"CGTCGAAA",
        b"CGTAAAAA",
        b"CGAAAAAA",
        b"AAAAAAAA",
    ] {
        session.undo(doc).unwrap();
        assert_eq!(session.alignment(doc).unwrap().read(0, 0, 8), expected);
    }
    assert!(!session.can_undo(doc).unwrap());
    for _ in 0..5 {
        session.redo(doc).unwrap();
    }
    assert_eq!(session.alignment(doc).unwrap().read(0, 0, 8), b"CGTCGTAC");
    session.save(doc, None).unwrap();
    drop(session);
    let entries = basecraft_ops::log::read_all(dir.path()).unwrap();
    let edits: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e.op, Op::AlnReplace { .. }))
        .collect();
    assert_eq!(edits.len(), 5);
    assert_eq!(edits[0].actor, first);
    assert_eq!(edits[1].actor, second);
    let report = basecraft_ops::records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
}
