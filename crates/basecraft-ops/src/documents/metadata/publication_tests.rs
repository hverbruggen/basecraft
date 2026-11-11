//! Verify actual partial files and retry behavior through Session.
use crate::{Session, metadata};

#[test]
fn partial_metadata_save_preserves_the_document_and_can_retry() {
    let Some(root) = crate::isolated_profile::enter(
        "documents::metadata::publication_tests::partial_metadata_save_preserves_the_document_and_can_retry",
    ) else {
        return;
    };
    let source = metadata::create(&root.join("source")).unwrap();
    let mut session = Session::new(&root);
    let id = session.open(&source).unwrap();
    // A real obstructing directory stops a table replacement after earlier tables.
    let target = root.join("copy").join(metadata::MANIFEST);
    let files = metadata::files(&target);
    std::fs::create_dir_all(&files[2]).unwrap();
    let original = std::fs::read(&source).unwrap();
    let before = session.save_hashes(id).unwrap();
    assert!(session.save(id, Some(&target)).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(session.save_hashes(id).unwrap(), before);
    assert!(files.iter().skip(3).all(|path| path.is_file()));
    assert!(!target.exists());
    std::fs::remove_dir(&files[2]).unwrap();
    session.save(id, Some(&target)).unwrap();
    let reopened = session.open(&target).unwrap();
    assert_eq!(session.save_hashes(reopened).unwrap(), before);
}
