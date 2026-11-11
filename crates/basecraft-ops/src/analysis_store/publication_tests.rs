use super::*;

#[test]
fn secondary_publication_is_complete_and_never_clobbers() {
    let root = tempfile::tempdir().unwrap();
    let run = ArtifactRun::create(&root.path().join("result.txt"), "test").unwrap();
    let source = run.artifact("secondary.txt").unwrap();
    std::fs::write(&source, b"retained result").unwrap();
    let destination = root.path().join("export.txt");
    run.publish_secondary("secondary.txt", &destination)
        .unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), b"retained result");
    std::fs::write(&source, b"new result").unwrap();
    assert!(
        run.publish_secondary("secondary.txt", &destination)
            .is_err()
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"retained result");
    assert_eq!(std::fs::read(&source).unwrap(), b"new result");
}

#[test]
fn failed_secondary_publication_retains_source() {
    let root = tempfile::tempdir().unwrap();
    let run = ArtifactRun::create(&root.path().join("result.txt"), "test").unwrap();
    let source = run.artifact("secondary.txt").unwrap();
    std::fs::write(&source, b"retained result").unwrap();
    let destination = root.path().join("missing/export.txt");
    assert!(
        run.publish_secondary("secondary.txt", &destination)
            .is_err()
    );
    assert!(!destination.exists());
    assert_eq!(std::fs::read(source).unwrap(), b"retained result");
}
