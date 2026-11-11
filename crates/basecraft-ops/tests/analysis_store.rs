#[path = "support/decided_save.rs"]
mod decided_save;
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::records::{ArtifactDestination, ArtifactRun};

#[test]
fn hidden_runs_reserve_stage_and_publish_without_replacing_files() {
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
    use basecraft_ops::records::{self, SubmittedInput};
    use basecraft_protocol::provenance::InputReference;
    let run = ArtifactRun::create(&root.join(format!("{id}.fasta")), id).unwrap();
    records::write_json(
        &run.directory().join("run.json"),
        &serde_json::json!({"result":format!("{id}.fasta")}),
    )
    .unwrap();
    let hash = basecraft_io::hash_file(source).unwrap();
    records::write_json(
        &run.directory().join("input.json"),
        &SubmittedInput {
            schema_version: 1,
            source: Some(InputReference {
                path: records::relative(run.directory(), source).unwrap(),
                stamp: records::stamp(source).unwrap(),
                hash: hash.clone(),
                extra: std::collections::BTreeMap::default(),
            }),
            submitted_hash: hash,
            submitted_size: std::fs::metadata(source).unwrap().len(),
            transformation: "canonical-fasta-v1".into(),
            state: "referenced".into(),
            analysis_run: None,
            proposal_target: None,
            upstream: Vec::new(),
            origin_run: None,
            extra: std::collections::BTreeMap::default(),
        },
    )
    .unwrap();
    run
}

#[test]
fn reference_lifecycle_multiple_analyses_preserve_break_trash_and_cancel() {
    use basecraft_ops::records::{self, SavePolicy};
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.fasta");
    let old = ">a\nAAAA\n>b\nAAAT\n";
    std::fs::write(&source, old).unwrap();
    let a = reference_run(dir.path(), &source, "one");
    let b = reference_run(dir.path(), &source, "two");
    let store = dir.path().join(".basecraft/analyses-v1");
    let conflict = records::conflicts_in(&source, std::slice::from_ref(&store)).unwrap();
    assert_eq!(conflict.analyses.len(), 2);
    assert!(records::resolve(&conflict, SavePolicy::Cancel).is_err());
    assert_eq!(std::fs::read_to_string(&source).unwrap(), old);
    assert!(!a.directory().join("preserved-input").exists());
    records::resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    std::fs::write(&source, ">a\nTTTT\n>b\nTTTT\n").unwrap();
    for run in [&a, &b] {
        let input = records::read_input(run.directory()).unwrap();
        assert_eq!(
            input.state,
            basecraft_protocol::input_records::EvidenceState::Preserved
        );
        let path =
            records::verified_source(run.directory(), input.source.as_ref().unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), old);
    }
    assert!(
        records::conflicts_in(&source, std::slice::from_ref(&store))
            .unwrap()
            .analyses
            .is_empty()
    );
    let c = reference_run(dir.path(), &source, "three");
    let conflict = records::conflicts_in(&source, std::slice::from_ref(&store)).unwrap();
    records::resolve(&conflict, SavePolicy::BreakEvidence).unwrap();
    let input = records::read_input(c.directory()).unwrap();
    assert!(records::verified_source(c.directory(), input.source.as_ref().unwrap()).is_err());
    let d = reference_run(dir.path(), &source, "four");
    let conflict = records::conflicts_in(&source, std::slice::from_ref(&store)).unwrap();
    records::resolve(&conflict, SavePolicy::RemoveAnalysis).unwrap();
    assert!(!d.directory().exists());
    assert_eq!(std::fs::read_dir(store.join("trash")).unwrap().count(), 1);
    assert!(source.exists());
    assert!(a.directory().exists());
}

#[test]
fn changed_missing_and_failed_copy_cannot_claim_preserved_evidence() {
    use basecraft_ops::records::{self, SavePolicy};
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.fasta");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let run = reference_run(dir.path(), &source, "run");
    let store = dir.path().join(".basecraft/analyses-v1");
    let conflict = records::conflicts_in(&source, std::slice::from_ref(&store)).unwrap();
    // A destination directory forces copying/publication failure; old input stays.
    std::fs::create_dir(run.directory().join("preserved-input")).unwrap();
    assert!(records::resolve(&conflict, SavePolicy::PreserveCopy).is_err());
    assert_eq!(
        records::read_input(run.directory()).unwrap().state,
        basecraft_protocol::input_records::EvidenceState::Referenced
    );
    std::fs::remove_dir(run.directory().join("preserved-input")).unwrap();
    std::fs::write(&source, ">a\nTTTT\n>b\nTTTT\n").unwrap();
    assert!(records::resolve(&conflict, SavePolicy::PreserveCopy).is_err());
    let input = records::read_input(run.directory()).unwrap();
    assert_eq!(
        input.state,
        basecraft_protocol::input_records::EvidenceState::Referenced
    );
    assert!(records::verified_source(run.directory(), input.source.as_ref().unwrap()).is_err());
    std::fs::remove_file(&source).unwrap();
    let missing = records::conflicts_in(&source, &[store]).unwrap();
    assert_eq!(missing.analyses.len(), 1);
    assert!(!missing.options.contains(&SavePolicy::PreserveCopy));
}

#[cfg(unix)]
#[test]
fn symlink_and_hardlink_aliases_are_detected_without_content_reads() {
    use basecraft_ops::records;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.fasta");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    reference_run(dir.path(), &source, "run");
    let store = dir.path().join(".basecraft/analyses-v1");
    let hard = dir.path().join("hard.fasta");
    std::fs::hard_link(&source, &hard).unwrap();
    assert_eq!(
        records::conflicts_in(&hard, std::slice::from_ref(&store))
            .unwrap()
            .analyses
            .len(),
        1
    );
    let soft = dir.path().join("soft.fasta");
    std::os::unix::fs::symlink(&source, &soft).unwrap();
    assert_eq!(
        records::conflicts_in(&soft.canonicalize().unwrap(), &[store])
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
        records::{self, SavePolicy},
    };
    crate::isolated_profile::isolate!();
    let mut fixtures = Vec::new();
    for policy in [
        SavePolicy::Cancel,
        SavePolicy::PreserveCopy,
        SavePolicy::RemoveAnalysis,
        SavePolicy::BreakEvidence,
    ] {
        let dir = tempfile::tempdir().unwrap();
        fixtures.push(dir);
        let dir = fixtures.last().unwrap();
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
                decided_save::save_with_policy(&mut session, doc, None, policy),
                Err(basecraft_ops::Error::EvidenceConflict(_))
            ));
            assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
        } else {
            if policy == SavePolicy::PreserveCopy {
                std::fs::create_dir(run.directory().join("preserved-input")).unwrap();
                assert!(decided_save::save_with_policy(&mut session, doc, None, policy).is_err());
                assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
                std::fs::remove_dir(run.directory().join("preserved-input")).unwrap();
            }
            decided_save::save_with_policy(&mut session, doc, None, policy).unwrap();
            assert!(std::fs::read_to_string(&source).unwrap().contains("TAAA"));
            match policy {
                SavePolicy::PreserveCopy => assert_eq!(
                    std::fs::read_to_string(run.directory().join("preserved-input")).unwrap(),
                    original
                ),
                SavePolicy::RemoveAnalysis => assert!(!run.directory().exists()),
                SavePolicy::BreakEvidence => assert_eq!(
                    records::read_input(run.directory()).unwrap().state,
                    basecraft_protocol::input_records::EvidenceState::Broken
                ),
                SavePolicy::Cancel => {}
            }
        }
    }
}

#[test]
fn export_and_extraction_cannot_bypass_known_input_guard() {
    crate::isolated_profile::isolate!();
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

#[test]
fn preserve_retries_and_failed_candidates_never_replace_retained_bytes() {
    use basecraft_ops::records::{self, SavePolicy};
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.afa");
    let original = b">a\nAAAA\n>b\nAAAT\n";
    std::fs::write(&source, original).unwrap();
    let run = reference_run(dir.path(), &source, "retry");
    let store = dir.path().join(".basecraft/analyses-v1");
    let conflict = records::conflicts_in(&source, &[store]).unwrap();
    std::fs::write(&source, b">a\nTTTT\n>b\nTTTA\n").unwrap();
    assert!(records::resolve(&conflict, SavePolicy::PreserveCopy).is_err());
    assert!(!run.directory().join("preserved-input").exists());
    std::fs::write(&source, original).unwrap();
    // Recover a verified copy whose ledger publication previously failed.
    std::fs::write(run.directory().join("preserved-input"), original).unwrap();
    records::resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    std::fs::remove_file(&source).unwrap();
    records::resolve(&conflict, SavePolicy::PreserveCopy).unwrap();
    assert_eq!(
        std::fs::read(run.directory().join("preserved-input")).unwrap(),
        original
    );
    // An invalid existing destination is retained for recovery, not replaced.
    let damaged = b"retained but damaged";
    std::fs::write(run.directory().join("preserved-input"), damaged).unwrap();
    assert!(records::resolve(&conflict, SavePolicy::PreserveCopy).is_err());
    assert_eq!(
        std::fs::read(run.directory().join("preserved-input")).unwrap(),
        damaged
    );
}

#[test]
fn nested_collection_bindings_reopen_without_changing_sibling_bindings() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("single.txt");
    let run = ArtifactRun::create(&output, "nested").unwrap();
    std::fs::write(&output, b"single").unwrap();
    run.bind(&output).unwrap();
    let collection = temp.path().join("collection/subdir");
    std::fs::create_dir_all(&collection).unwrap();
    let member = collection.join("member.txt");
    std::fs::write(&member, b"member").unwrap();
    run.bind(&member).unwrap();
    for path in [&output, &member] {
        assert_eq!(
            ArtifactRun::for_result(path).unwrap().as_deref(),
            Some(run.directory())
        );
    }
    // A same-named unrelated sibling must not accidentally inherit the binding.
    let other = temp.path().join("other/member.txt");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&other, b"unrelated").unwrap();
    assert!(ArtifactRun::for_result(&other).unwrap().is_none());
    // Discovery retains access to the ledger after edits so the provenance
    // verifier can diagnose changed bytes rather than losing the record.
    std::fs::write(&member, b"changed").unwrap();
    assert_eq!(
        ArtifactRun::for_result(&member).unwrap().as_deref(),
        Some(run.directory())
    );
}

#[test]
fn multi_input_verification_uses_only_declared_owners_and_never_bypasses_breaks() {
    use basecraft_ops::records;
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let source_a = temp.path().join("a.txt");
    let source_b = temp.path().join("b.txt");
    std::fs::write(&source_a, b"first").unwrap();
    std::fs::write(&source_b, b"second").unwrap();
    let make_ledger = |source: &std::path::Path| {
        records::reference_input_with_dependencies(
            &temp.path().join("result.txt"),
            source,
            &basecraft_io::hash_file(source).unwrap(),
            &[],
            "test input",
        )
        .unwrap()
    };
    let a = make_ledger(&source_a);
    let b = make_ledger(&source_b);
    let same_hash = make_ledger(&source_b);
    let main = ArtifactRun::create(&temp.path().join("result.txt"), "multi-source").unwrap();
    let dependencies = [
        a.directory().to_owned(),
        b.directory().to_owned(),
        same_hash.directory().to_owned(),
    ];
    records::link_generated_input(
        &main,
        &dependencies,
        b"submitted view",
        "identity-v1",
        "test",
    )
    .unwrap();
    let reference = records::read_input(b.directory()).unwrap().source.unwrap();
    assert_eq!(
        records::verified_source(main.directory(), &reference)
            .unwrap()
            .canonicalize()
            .unwrap(),
        source_b
    );
    let mut wrong_size = reference.clone();
    wrong_size.stamp.size += 1;
    assert!(records::verified_source(main.directory(), &wrong_size).is_err());
    let mut broken = records::read_input(same_hash.directory()).unwrap();
    broken.state = "broken".into();
    records::write_json(&same_hash.directory().join("input.json"), &broken).unwrap();
    for dependencies in [
        dependencies.to_vec(),
        dependencies.into_iter().rev().collect(),
    ] {
        records::link_generated_input(
            &main,
            &dependencies,
            b"submitted view",
            "identity-v1",
            "test",
        )
        .unwrap();
        assert!(
            records::verified_source(main.directory(), &reference)
                .unwrap_err()
                .to_string()
                .contains("explicitly broken")
        );
    }
    // A mutable ledger with the right hash is insufficient: its verified,
    // immutable publication must also declare that reference.
    let undeclared = make_ledger(&source_a);
    let mut altered = records::read_input(undeclared.directory()).unwrap();
    altered.source = Some(reference.clone());
    records::write_json(&undeclared.directory().join("input.json"), &altered).unwrap();
    records::link_generated_input(
        &main,
        &[undeclared.directory().to_owned()],
        b"submitted view",
        "identity-v1",
        "test",
    )
    .unwrap();
    assert!(records::verified_source(main.directory(), &reference).is_err());
}

#[test]
fn preserved_primary_is_not_replaced_by_a_matching_ancestor_with_missing_original() {
    use basecraft_ops::records;
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("original.txt");
    let bytes = b"retained primary bytes";
    std::fs::write(&source, bytes).unwrap();
    let hash = basecraft_io::hash_file(&source).unwrap();
    let anchor = temp.path().join("result.txt");
    let ancestor =
        records::reference_input_with_dependencies(&anchor, &source, &hash, &[], "ancestor")
            .unwrap();
    let main = records::reference_input_with_dependencies(
        &anchor,
        &source,
        &hash,
        &[ancestor.directory().to_owned()],
        "primary",
    )
    .unwrap();
    // Preserve this primary while the ancestor still references the original.
    // Matching ancestry is inspected for explicit breaks, not substituted for
    // a primary whose identity and preserved bytes remain valid.
    let mut input = records::read_input(main.directory()).unwrap();
    let reference = input.source.clone().unwrap();
    let preserved = main.directory().join("preserved-input");
    std::fs::write(&preserved, bytes).unwrap();
    input.state = "preserved".into();
    records::write_json(&main.directory().join("input.json"), &input).unwrap();
    std::fs::remove_file(&source).unwrap();
    assert_eq!(
        records::verified_source(main.directory(), &reference).unwrap(),
        preserved
    );
    assert_eq!(std::fs::read(&preserved).unwrap(), bytes);
    // A matching explicit break still fails closed, even with a good primary.
    let mut broken = records::read_input(ancestor.directory()).unwrap();
    broken.state = "broken".into();
    records::write_json(&ancestor.directory().join("input.json"), &broken).unwrap();
    assert!(
        records::verified_source(main.directory(), &reference)
            .unwrap_err()
            .to_string()
            .contains("explicitly broken")
    );
}
