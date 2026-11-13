#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{Session, records};
use basecraft_protocol::{Feature, Location, Op, Strand};

#[test]
fn new_sequence_validates_before_creating_and_replays_unsaved_edits() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let profile = tempfile::tempdir().unwrap();
    let paths = basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| profile.path().as_os_str().to_owned())
    })
    .unwrap();
    let mut session = Session::recoverable_with_paths(root.path(), &paths).unwrap();
    for (name, text) in [
        ("", "ACGT"),
        ("two names", "ACGT"),
        ("a", ""),
        ("a", "AC?"),
        ("a", "ACGU"),
        ("a", "PEPTIDE"),
        ("a", ">a\nACGT"),
    ] {
        assert!(
            session.create_sequence(name, text).is_err(),
            "{name}: {text}"
        );
    }
    assert!(!session.recovery_root().exists());
    let id = session
        .create_sequence("sample", "ac gt\nryswkmbdhvn")
        .unwrap();
    assert_eq!(id.0, 0);
    assert_eq!(session.doc(id).unwrap().read(0, 15), b"ACGTRYSWKMBDHVN");
    assert!(session.document_record(id).unwrap().unsaved);
    assert!(!root.path().join(session.doc_key(id).unwrap()).exists());
    assert!(session.save(id, None).is_err());
    // Cancelling a proposed annotation does not apply an operation.
    let before = session.record_revision(id).unwrap();
    let feature = Feature {
        kind: "misc_feature".into(),
        location: Location::simple(1, 4, Strand::Reverse),
        qualifiers: vec![],
    };
    assert_eq!(session.record_revision(id).unwrap(), before);
    session
        .apply(
            id,
            Op::FeatureAdd {
                feature,
                record: None,
            },
        )
        .unwrap();
    session.undo(id).unwrap();
    assert_eq!(session.doc(id).unwrap().features().count(), 0);
    session.redo(id).unwrap();
    assert_eq!(
        session
            .doc(id)
            .unwrap()
            .features()
            .next()
            .unwrap()
            .1
            .location
            .strand(),
        Strand::Reverse
    );
    let recovery = session.recovery_root().to_owned();
    assert_eq!(
        records::replay(&recovery).unwrap().problems,
        Vec::<String>::new()
    );
    let restored = root.path().join("restored");
    let outputs = records::recover(&recovery, &restored).unwrap();
    let mut recovered = Session::unlogged(&restored);
    let copy = recovered.open(&outputs[0]).unwrap();
    assert_eq!(
        recovered.doc(copy).unwrap().read(0, 15),
        session.doc(id).unwrap().read(0, 15)
    );
    assert_eq!(recovered.doc(copy).unwrap().features().count(), 1);
    let output = root.path().join("saved.gb");
    session.save_new(id, Some(&output)).unwrap();
    let reopened = session.open(&output).unwrap();
    assert!(!session.document_record(reopened).unwrap().unsaved);
    assert_eq!(
        session.doc(reopened).unwrap().read(0, 15),
        b"ACGTRYSWKMBDHVN"
    );
    assert!(session.save_new(id, Some(&output)).is_err());
}
