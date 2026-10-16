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

fn reference_run(root: &std::path::Path, source: &std::path::Path, id: &str) -> ArtifactRun {
    use basecraft_ops::input_evidence::{self, SubmittedInput};
    use basecraft_protocol::provenance::InputReference;
    let run = ArtifactRun::create(&root.join(format!("{id}.fasta")), id).unwrap();
    input_evidence::write_json(
        &run.directory().join("run.json"),
        &serde_json::json!({"result":format!("{id}.fasta")}),
    )
    .unwrap();
    let hash = basecraft_io::hash_file(source).unwrap();
    input_evidence::write_json(
        &run.directory().join("input.json"),
        &SubmittedInput {
            schema_version: 1,
            source: Some(InputReference {
                path: input_evidence::relative(run.directory(), source).unwrap(),
                stamp: input_evidence::stamp(source).unwrap(),
                hash: hash.clone(),
                extra: Default::default(),
            }),
            submitted_hash: hash,
            submitted_size: std::fs::metadata(source).unwrap().len(),
            transformation: "canonical-fasta-v1".into(),
            state: "referenced".into(),
            extra: Default::default(),
        },
    )
    .unwrap();
    run
}

#[test]
fn reference_lifecycle_multiple_analyses_preserve_break_trash_and_cancel() {
    use basecraft_ops::input_evidence::{self, SavePolicy};
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.fasta");
    let old = ">a\nAAAA\n>b\nAAAT\n";
    std::fs::write(&source, old).unwrap();
    let a = reference_run(dir.path(), &source, "one");
    let b = reference_run(dir.path(), &source, "two");
    let store = dir.path().join(".basecraft/analyses-v1");
    let conflict = input_evidence::conflicts_in(&source, std::slice::from_ref(&store)).unwrap();
    assert_eq!(conflict.analyses.len(), 2);
    assert!(input_evidence::resolve(&conflict, SavePolicy::Cancel).is_err());
    assert_eq!(std::fs::read_to_string(&source).unwrap(), old);
    assert!(!a.directory().join("preserved-input").exists());
    input_evidence::resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    std::fs::write(&source, ">a\nTTTT\n>b\nTTTT\n").unwrap();
    for run in [&a, &b] {
        let input = input_evidence::read_input(run.directory()).unwrap();
        assert_eq!(input.state, "preserved");
        let path = input_evidence::verified_source(run.directory(), input.source.as_ref().unwrap())
            .unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), old);
    }
    assert!(
        input_evidence::conflicts_in(&source, std::slice::from_ref(&store))
            .unwrap()
            .analyses
            .is_empty()
    );
    let c = reference_run(dir.path(), &source, "three");
    let conflict = input_evidence::conflicts_in(&source, std::slice::from_ref(&store)).unwrap();
    input_evidence::resolve(&conflict, SavePolicy::BreakEvidence).unwrap();
    let input = input_evidence::read_input(c.directory()).unwrap();
    assert!(
        input_evidence::verified_source(c.directory(), input.source.as_ref().unwrap()).is_err()
    );
    let d = reference_run(dir.path(), &source, "four");
    let conflict = input_evidence::conflicts_in(&source, &[store.clone()]).unwrap();
    input_evidence::resolve(&conflict, SavePolicy::RemoveAnalysis).unwrap();
    assert!(!d.directory().exists());
    assert_eq!(std::fs::read_dir(store.join("trash")).unwrap().count(), 1);
    assert!(source.exists());
    assert!(a.directory().exists());
}

#[test]
fn changed_missing_and_failed_copy_cannot_claim_preserved_evidence() {
    use basecraft_ops::input_evidence::{self, SavePolicy};
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.fasta");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let run = reference_run(dir.path(), &source, "run");
    let store = dir.path().join(".basecraft/analyses-v1");
    let conflict = input_evidence::conflicts_in(&source, std::slice::from_ref(&store)).unwrap();
    // A destination directory forces copying/publication failure; old input stays.
    std::fs::create_dir(run.directory().join("preserved-input")).unwrap();
    assert!(input_evidence::resolve(&conflict, SavePolicy::PreserveCopy).is_err());
    assert_eq!(
        input_evidence::read_input(run.directory()).unwrap().state,
        "referenced"
    );
    std::fs::remove_dir(run.directory().join("preserved-input")).unwrap();
    std::fs::write(&source, ">a\nTTTT\n>b\nTTTT\n").unwrap();
    assert!(input_evidence::resolve(&conflict, SavePolicy::PreserveCopy).is_err());
    let input = input_evidence::read_input(run.directory()).unwrap();
    assert_eq!(input.state, "referenced");
    assert!(
        input_evidence::verified_source(run.directory(), input.source.as_ref().unwrap()).is_err()
    );
    std::fs::remove_file(&source).unwrap();
    let missing = input_evidence::conflicts_in(&source, &[store]).unwrap();
    assert_eq!(missing.analyses.len(), 1);
    assert!(!missing.options.contains(&SavePolicy::PreserveCopy));
}

#[cfg(unix)]
#[test]
fn symlink_and_hardlink_aliases_are_detected_without_content_reads() {
    use basecraft_ops::input_evidence;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.fasta");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    reference_run(dir.path(), &source, "run");
    let store = dir.path().join(".basecraft/analyses-v1");
    let hard = dir.path().join("hard.fasta");
    std::fs::hard_link(&source, &hard).unwrap();
    assert_eq!(
        input_evidence::conflicts_in(&hard, std::slice::from_ref(&store))
            .unwrap()
            .analyses
            .len(),
        1
    );
    let soft = dir.path().join("soft.fasta");
    std::os::unix::fs::symlink(&source, &soft).unwrap();
    assert_eq!(
        input_evidence::conflicts_in(&soft.canonicalize().unwrap(), &[store])
            .unwrap()
            .analyses
            .len(),
        1
    );
}

#[test]
fn session_save_policies_and_copy_failure_leave_source_safe() {
    use basecraft_ops::{
        Session,
        input_evidence::{self, SavePolicy},
    };
    for policy in [
        SavePolicy::Cancel,
        SavePolicy::PreserveCopy,
        SavePolicy::RemoveAnalysis,
        SavePolicy::BreakEvidence,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.afa");
        let original = ">a\nAAAA\n>b\nAAAT\n";
        std::fs::write(&source, original).unwrap();
        let mut session = Session::recoverable(dir.path()).unwrap();
        let doc = session.open(&source).unwrap();
        let run = reference_run(dir.path(), &source, "run");
        session
            .apply(
                doc,
                basecraft_protocol::Op::AlnReplace {
                    row: 0,
                    start: 0,
                    end: 1,
                    residues: "T".into(),
                },
            )
            .unwrap();
        if policy == SavePolicy::Cancel {
            assert!(matches!(
                session.save_with_evidence(doc, None, policy),
                Err(basecraft_ops::Error::EvidenceConflict(_))
            ));
            assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
        } else {
            if policy == SavePolicy::PreserveCopy {
                std::fs::create_dir(run.directory().join("preserved-input")).unwrap();
                assert!(session.save_with_evidence(doc, None, policy).is_err());
                assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
                std::fs::remove_dir(run.directory().join("preserved-input")).unwrap();
            }
            session.save_with_evidence(doc, None, policy).unwrap();
            assert!(std::fs::read_to_string(&source).unwrap().contains("TAAA"));
            match policy {
                SavePolicy::PreserveCopy => assert_eq!(
                    std::fs::read_to_string(run.directory().join("preserved-input")).unwrap(),
                    original
                ),
                SavePolicy::RemoveAnalysis => assert!(!run.directory().exists()),
                SavePolicy::BreakEvidence => assert_eq!(
                    input_evidence::read_input(run.directory()).unwrap().state,
                    "broken"
                ),
                SavePolicy::Cancel => {}
            }
        }
    }
}

#[test]
fn export_and_extraction_cannot_bypass_known_input_guard() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("sequence.fasta");
    let original = ">example\nACGTACGT\n";
    std::fs::write(&source, original).unwrap();
    let mut session = basecraft_ops::Session::recoverable(root.path()).unwrap();
    let doc = session.open(&source).unwrap();
    reference_run(root.path(), &source, "analysis");
    let mut wrote = false;
    let export = session.export(doc, &source, "fasta", |writer| {
        wrote = true;
        writer.write_all(b"replacement")
    });
    assert!(matches!(
        export,
        Err(basecraft_ops::Error::EvidenceConflict(_))
    ));
    assert!(!wrote);
    assert!(matches!(
        session.extract(doc, (0, 4), &[], false, &source),
        Err(basecraft_ops::Error::EvidenceConflict(_))
    ));
    assert_eq!(std::fs::read_to_string(source).unwrap(), original);
}
