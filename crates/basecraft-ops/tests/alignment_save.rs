//! Ordinary saves must not turn ragged input into an asserted alignment.
use basecraft_ops::Session;
use basecraft_protocol::Op;

const RAGGED: &str = ">a long description\nACGT--\n>b short description\nA.-\n";

#[test]
fn ragged_save_and_save_as_reopen_preserve_explicit_lengths_and_gaps() {
    for save_as in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.afa");
        let destination = dir.path().join("copy.afa");
        std::fs::write(&source, RAGGED).unwrap();
        let target = if save_as { &destination } else { &source };
        {
            let mut session = Session::new(dir.path());
            let doc = session.open(&source).unwrap();
            assert!(session.alignment(doc).validate_aligned_input().is_err());
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
            assert!(session.alignment(doc).validate_aligned_input().is_err());
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
        let alignment = reopened.alignment(doc);
        assert!(alignment.validate_aligned_input().is_err());
        assert_eq!(alignment.saved_row_len(1), 3);
        assert_eq!(alignment.read(1, 0, 6), b"T.----");
    }
}

#[test]
fn ragged_extent_tracks_explicit_gap_edits_undo_redo_and_snapshots() {
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
        assert_eq!(session.alignment(doc).saved_row_len(1), 5);
        assert!(session.alignment(doc).validate_aligned_input().is_err());
        session.undo(doc).unwrap();
        assert_eq!(session.alignment(doc).saved_row_len(1), 3);
        session.redo(doc).unwrap();
        assert_eq!(session.alignment(doc).snapshot().saved_row_len(1), 5);
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
        assert!(session.alignment(doc).validate_aligned_input().is_ok());
        session.undo(doc).unwrap();
        assert!(session.alignment(doc).validate_aligned_input().is_err());
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        RAGGED.replace("A.-\n", "A.----\n")
    );
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    assert!(session.alignment(doc).validate_aligned_input().is_ok());
    assert_eq!(session.alignment(doc).read(1, 0, 6), b"A.----");
}

#[test]
fn ragged_row_extents_follow_reordering_and_undo_after_width_growth() {
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
        assert_eq!(session.alignment(doc).width(), 7);
        assert_eq!(session.alignment(doc).saved_row_len(1), 3);
        session
            .apply(
                doc,
                Op::AlnMoveRows {
                    rows: vec![(0, 1)],
                    to: 1,
                },
            )
            .unwrap();
        assert_eq!(session.alignment(doc).saved_row_len(0), 3);
        session.undo(doc).unwrap();
        assert_eq!(session.alignment(doc).saved_row_len(1), 3);
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        ">b short description\nA.-\n>a long description\nNACGT--\n"
    );
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    assert!(session.alignment(doc).validate_aligned_input().is_err());
}

#[test]
fn equal_alignment_preserves_literal_terminal_gaps_and_existing_delete_semantics() {
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
        assert_eq!(session.alignment(doc).read(0, 0, 6), b"C--..-");
        assert!(session.alignment(doc).validate_aligned_input().is_ok());
        session.undo(doc).unwrap();
        assert_eq!(session.alignment(doc).read(0, 0, 6), b"AC--..");
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        ">a description\nC--..-\n>b\nA-CG--\n"
    );
    let mut session = Session::new(dir.path());
    let doc = session.open(&source).unwrap();
    assert!(session.alignment(doc).validate_aligned_input().is_ok());
}
