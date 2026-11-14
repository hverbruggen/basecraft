use basecraft_ops::{Session, records};
use basecraft_protocol::Op;

#[test]
fn compact_alignment_unicode_path_edit_undo_save_reopen_replay() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("homologues α copy.afa");
    std::fs::write(
        &path,
        include_bytes!("../../../../testdata/fixtures/compact/alignment-editing/homologues.afa"),
    )
    .unwrap();
    {
        let mut session = Session::new(dir.path());
        let doc = session.open(&path).unwrap();
        assert!(session.is_alignment(doc));
        assert_eq!(session.alignment(doc).unwrap().row_count(), 8);
        assert_eq!(session.alignment(doc).unwrap().width(), 180);
        let original = session.alignment(doc).unwrap().read(0, 0, 180);
        session
            .apply(
                doc,
                Op::AlnReplace {
                    row: 0,
                    start: 0,
                    end: 3,
                    residues: "NNN".into(),
                },
            )
            .unwrap();
        assert_eq!(session.alignment(doc).unwrap().read(0, 0, 3), b"NNN");
        session.undo(doc).unwrap();
        assert_eq!(session.alignment(doc).unwrap().read(0, 0, 180), original);
        session.redo(doc).unwrap();
        session.save(doc, None).unwrap();
    }
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:?}", report.problems);
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    assert_eq!(session.alignment(doc).unwrap().read(0, 0, 3), b"NNN");
    for row in 0..8 {
        assert_eq!(
            session.alignment(doc).unwrap().row(row).name,
            format!("specimen_{:02}", row + 1)
        );
    }
}
