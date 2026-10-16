use basecraft_ops::analysis_store::{ArtifactDestination, ArtifactRun};

#[test]
fn hidden_runs_reserve_stage_and_publish_without_replacing_files() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("result.nwk");
    let run = ArtifactRun::create(&output, "run-1").unwrap();
    assert!(!temp.path().join(".basecraft/project.toml").exists());
    assert!(ArtifactRun::create(&output, "run-1").is_err());
    assert!(run.artifact("../escape").is_err());
    assert!(run.artifact("/escape").is_err());
    std::fs::write(run.staging("bootstrap.nex").unwrap(), "replicates").unwrap();
    let hidden = run
        .publish("bootstrap.nex", ArtifactDestination::default())
        .unwrap();
    assert_eq!(std::fs::read_to_string(hidden).unwrap(), "replicates");
    std::fs::write(run.staging("bootstrap.nex").unwrap(), "replacement").unwrap();
    assert!(
        run.publish("bootstrap.nex", ArtifactDestination::Metadata)
            .is_err()
    );
    assert!(run.staging("bootstrap.nex").unwrap().is_file());
    std::fs::write(run.staging("visible.nex").unwrap(), "explicit export").unwrap();
    assert_eq!(
        run.publish("visible.nex", ArtifactDestination::WorkingDirectory)
            .unwrap(),
        temp.path().join("visible.nex")
    );
    assert!(!temp.path().join("bootstrap.nex").exists());
}

#[test]
fn foreign_namespace_is_never_adopted_or_migrated() {
    let temp = tempfile::tempdir().unwrap();
    let namespace = temp.path().join(".basecraft/analyses-v1");
    std::fs::create_dir_all(&namespace).unwrap();
    std::fs::write(namespace.join("user-data"), "untouched").unwrap();
    assert!(ArtifactRun::create(&temp.path().join("out"), "run").is_err());
    assert_eq!(
        std::fs::read_to_string(namespace.join("user-data")).unwrap(),
        "untouched"
    );
    assert!(!namespace.join("store.json").exists());
}

#[cfg(unix)]
#[test]
fn symlink_namespace_and_staged_symlinks_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), temp.path().join(".basecraft")).unwrap();
    assert!(ArtifactRun::create(&temp.path().join("out"), "run").is_err());
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    std::fs::remove_file(temp.path().join(".basecraft")).unwrap();
    let run = ArtifactRun::create(&temp.path().join("out"), "run").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", run.staging("bad").unwrap()).unwrap();
    assert!(run.publish("bad", ArtifactDestination::Metadata).is_err());
}
