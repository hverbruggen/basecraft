#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{Session, files};
use basecraft_protocol::{Op, SupportKind};

#[test]
fn same_path_replacement_cannot_reuse_tree_metadata() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "((a,b)90,c);\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    session
        .apply(
            doc,
            Op::TreeAddSupport {
                name: "Imported".into(),
                source: "source".into(),
                values: vec![None, Some("42".into())],
                kind: SupportKind::default(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    std::fs::write(&path, "((c,b)10,a);\n").unwrap();
    assert!(
        files::load_tree(&path).unwrap().metadata_warning.is_some(),
        "stale metadata was accepted"
    );
}

#[test]
fn undone_import_is_absent_from_methods() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("tree.nwk");
    let source = dir.path().join("source.nwk");
    std::fs::write(&target, "((a,b)90,c);\n").unwrap();
    std::fs::write(&source, "((a,b)42,c);\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&target).unwrap();
    session.import_support(doc, &source, "Imported").unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(
        report.methods.contains("import_support"),
        "retained import absent from Methods"
    );
    session.undo(doc).unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(!report.methods.contains("import_support"));
}

fn paths(root: &std::path::Path) -> basecraft_platform::paths::AppPaths {
    basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| root.join("profile").into_os_string())
    })
    .unwrap()
}

#[test]
fn unversioned_tree_metadata_is_ignored_without_changing_files() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "(a,b);").unwrap();
    let sidecar = basecraft_ops::files::tree_sidecar(&path);
    let bytes = br#"{"supports":[],"annotations":{},"style":{}}"#;
    std::fs::write(&sidecar, bytes).unwrap();
    assert!(
        basecraft_ops::files::load_tree(&path)
            .unwrap()
            .metadata_warning
            .is_some()
    );
    assert_eq!(std::fs::read(sidecar).unwrap(), bytes);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "(a,b);");
}

#[test]
fn hidden_binding_errors_never_fall_back_to_adjacent_metadata() {
    use basecraft_ops::records::ArtifactRun;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forest.nwk");
    std::fs::write(&path, "((a,b)90,c);\n(a,b,c);\n").unwrap();
    let tree = files::load_tree(&path).unwrap().doc;
    let mut metadata = Vec::new();
    files::tree_companion(&tree, &basecraft_io::hash_file(&path).unwrap())
        .write(&mut metadata)
        .unwrap();
    std::fs::write(files::tree_sidecar(&path), &metadata).unwrap();
    let run = ArtifactRun::create(&path, "metadata-test").unwrap();
    std::fs::write(
        run.artifact("forest.nwk.basecraft.json").unwrap(),
        &metadata,
    )
    .unwrap();
    run.bind(&path).unwrap();
    assert!(files::load_tree(&path).is_ok());
    let sidecar = run.artifact("forest.nwk.basecraft.json").unwrap();
    std::fs::remove_file(&sidecar).unwrap();
    let missing = files::load_tree(&path).unwrap();
    assert!(
        missing
            .metadata_warning
            .as_deref()
            .unwrap()
            .contains("unavailable")
    );
    std::fs::write(&sidecar, &metadata).unwrap();
    let mut future: serde_json::Value = serde_json::from_slice(&metadata).unwrap();
    future["version"] = serde_json::json!(99);
    std::fs::write(&sidecar, serde_json::to_vec(&future).unwrap()).unwrap();
    assert!(files::load_tree(&path).unwrap().metadata_warning.is_some());
    std::fs::write(&sidecar, &metadata).unwrap();
    let binding = std::fs::read_dir(dir.path().join(".basecraft/analyses-v1/bindings"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original = std::fs::read(&binding).unwrap();
    for broken in [b"{".as_slice(), br#"{"schema_version":42}"#.as_slice()] {
        std::fs::write(&binding, broken).unwrap();
        assert!(files::load_tree(&path).unwrap().metadata_warning.is_some());
    }
    std::fs::write(&binding, original).unwrap();
    // The first tree still matches: the full forest binding must detect this.
    std::fs::write(&path, "((a,b)90,c);\n(x,y,z);\n").unwrap();
    assert!(files::load_tree(&path).unwrap().metadata_warning.is_some());
    assert!(
        ArtifactRun::for_result(&path).unwrap().is_some(),
        "writer lookup must remain available"
    );
}

#[test]
fn imported_support_partial_removal_undo_save_reopen_and_redo() {
    use basecraft_ops::records;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("tree.nwk");
    let source = dir.path().join("source.nwk");
    std::fs::write(&target, "((a,b)90,c);\n").unwrap();
    std::fs::write(&source, "((a,b)42/88,c);\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&target).unwrap();
    let imported = session.import_support(doc, &source, "Imported").unwrap();
    session.save(doc, None).unwrap();
    session
        .apply(
            doc,
            Op::TreeRemoveSupport {
                name: imported.added[0].clone(),
            },
        )
        .unwrap();
    assert!(
        session
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .methods
            .contains("import_support")
    );
    session.save(doc, None).unwrap();
    session
        .apply(
            doc,
            Op::TreeRemoveSupport {
                name: imported.added[1].clone(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert!(
        !session
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .methods
            .contains("import_support")
    );
    let input = basecraft_ops::records::read_input(&records::bundle_for_result(&target)).unwrap();
    assert_eq!(
        serde_json::to_value(&input.upstream)
            .unwrap()
            .as_array()
            .unwrap()
            .as_slice(),
        [] as [serde_json::Value; 0]
    );
    session.undo(doc).unwrap();
    session.save(doc, None).unwrap();
    assert!(
        session
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .methods
            .contains("import_support")
    );
    session.redo(doc).unwrap();
    session.save(doc, None).unwrap();
    drop(session);
    let mut reopened = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = reopened.open(&target).unwrap();
    assert!(
        !reopened
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .methods
            .contains("import_support")
    );
    std::fs::write(&source, "((a,b)0/0,c);\n").unwrap();
    assert!(
        records::verify(&target).unwrap().is_empty(),
        "removed source still linked"
    );
}

fn annotation(start: u64) -> Op {
    Op::FeatureAdd {
        feature: basecraft_protocol::Feature {
            kind: "CDS".into(),
            location: basecraft_protocol::Location::simple(
                start,
                start + 2,
                basecraft_protocol::Strand::Forward,
            ),
            qualifiers: Vec::new(),
        },
    }
}

fn annotate(session: &mut Session, doc: basecraft_ops::DocId, label: &str, starts: &[u64]) {
    let actor = basecraft_protocol::Actor::Plugin {
        id: label.into(),
        version: "1".into(),
    };
    let ops: Vec<_> = starts.iter().map(|s| annotation(*s)).collect();
    let ids = session
        .apply_sequence_proposals(
            doc,
            session.generation(doc).unwrap(),
            session.revision(doc).unwrap(),
            &ops,
            &actor,
        )
        .unwrap();
    session
        .log_contributing_tool_run(
            doc,
            actor,
            (label, "annotate"),
            serde_json::json!({"effective":{"threshold":7}}),
            &[],
            &ids,
        )
        .unwrap();
}

#[test]
fn annotation_runs_follow_partial_deletion_and_feature_ids_after_reopen() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGTACGTACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&file).unwrap();
    annotate(&mut session, doc, "first-plugin", &[0, 4]);
    annotate(&mut session, doc, "second-plugin", &[8]);
    session.apply(doc, Op::FeatureRemove { id: 0 }).unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(report.methods.contains("first-plugin") && report.methods.contains("second-plugin"));
    let saved = dir.path().join("saved.fasta");
    session.save(doc, Some(&saved)).unwrap();
    drop(session);
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&saved).unwrap();
    // Serialization compacted the deleted slot: first-plugin's survivor is ID 0.
    session.apply(doc, Op::FeatureRemove { id: 0 }).unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(
        !report.methods.contains("first-plugin"),
        "{}",
        report.methods
    );
    assert!(report.methods.contains("second-plugin"));
    session.undo(doc).unwrap();
    assert!(
        session
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .methods
            .contains("first-plugin")
    );
    session.redo(doc).unwrap();
    session.apply(doc, Op::FeatureRemove { id: 1 }).unwrap();
    assert!(
        !session
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .methods
            .contains("second-plugin")
    );
    let record = session.document_record(doc).unwrap();
    assert!(!record.effective.iter().any(|edit| matches!(
        edit.operation,
        Op::FeatureAdd { .. } | Op::FeatureUpdate { .. }
    )));
    assert!(
        record
            .events
            .iter()
            .any(|event| matches!(event.op, Op::FeatureAdd { .. }))
    );
}

#[test]
fn generic_no_output_activity_does_not_own_unrelated_manual_edits() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gene.fasta");
    std::fs::write(&file, ">gene\nACGTACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session.begin_group();
    session.apply(doc, annotation(0)).unwrap();
    session.end_group();
    session
        .log_tool_run(
            doc,
            basecraft_protocol::Actor::Human {
                user: basecraft_platform::username(),
            },
            ("unrelated-plugin", "inspect"),
            serde_json::json!({}),
            &[],
            &[],
        )
        .unwrap();
    assert!(
        !session
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .methods
            .contains("unrelated-plugin")
    );
}

#[test]
fn extraction_is_reference_first_and_packages_exact_source_and_mapping() {
    use basecraft_ops::records;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("gene.fasta");
    let result = dir.path().join("extract.gb");
    std::fs::write(&source, ">gene\nACGTACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&source).unwrap();
    session
        .extract(doc, (1, 7), &[(1, 3), (5, 7)], true, &result)
        .unwrap();
    let publication = records::inspect(&result).unwrap();
    let conflicts = basecraft_ops::records::conflicts(&source).unwrap();
    assert!(
        !conflicts.analyses.is_empty(),
        "extraction did not register its referenced source in shared save protection"
    );
    assert!(
        conflicts
            .analyses
            .iter()
            .any(|analysis| analysis.result == "extract.gb")
    );
    assert!(publication.inputs[0].retained.is_none());
    assert!(publication.inputs[0].reference.is_some());
    let issues = records::verify(&result).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    let input = basecraft_ops::records::read_input(&records::bundle_for_result(&result)).unwrap();
    assert_eq!(
        input.extra["extraction"]["selection"]["params"]["segments"],
        serde_json::json!([[1, 3], [5, 7]])
    );
    let package = dir.path().join("package");
    records::package(&result, &package).unwrap();
    std::fs::write(&source, ">gene\nTTTTTTTT\n").unwrap();
    assert_ne!(records::verify(&result).unwrap(), Vec::<String>::new());
    assert_eq!(
        records::verify(&package.join("extract.gb")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn saving_a_forest_preserves_other_members_and_refreshes_bound_identity() {
    use basecraft_ops::records::ArtifactRun;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("forest.nwk");
    std::fs::write(&file, "((a,b)90,c);\n((x,y)42,z);\n").unwrap();
    let run = ArtifactRun::create(&file, "forest-save").unwrap();
    let tree = files::load_tree(&file).unwrap().doc;
    let mut metadata = Vec::new();
    files::tree_companion(&tree, &basecraft_io::hash_file(&file).unwrap())
        .write(&mut metadata)
        .unwrap();
    std::fs::write(run.artifact("forest.nwk.basecraft.json").unwrap(), metadata).unwrap();
    run.bind(&file).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session
        .apply(
            doc,
            Op::TreeRenameTip {
                from: "a".into(),
                to: "renamed-a".into(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert!(ArtifactRun::for_result_checked(&file).unwrap().is_some());
    let opened = files::load_tree(&file).unwrap();
    assert_eq!(opened.remaining.len(), 1);
    assert_eq!(
        opened.remaining[0].nodes,
        basecraft_io::newick::parse_newick("((x,y)42,z);")
            .unwrap()
            .nodes
    );
    assert!(opened.doc.to_newick().contains("renamed-a"));
    session.undo(doc).unwrap();
    session.save(doc, None).unwrap();
    assert!(
        !files::load_tree(&file)
            .unwrap()
            .doc
            .to_newick()
            .contains("renamed-a")
    );
}

#[test]
fn nexus_forest_retains_member_identity_rooting_and_untouched_annotations() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("forest.nex");
    let text = "#NEXUS\nbegin trees;\ntranslate 1 a, 2 b, 3 c;\ntree 'selected tree' = [&U] ((1:1,2:2)90:3,3:4);\ntree 'rooted member' = [&R] ((1:1,3:2)[&prob=0.99,note=x]:3,2:4);\nutree 'unrooted member' = ((2:5,3:6),1);\ntree 'unknown member' = (1:1,2:2,3:3);\nend;\n";
    std::fs::write(&file, text).unwrap();
    let before = basecraft_io::newick::parse_trees(text).unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&file).unwrap();
    session
        .apply(
            doc,
            Op::TreeRenameTip {
                from: "a".into(),
                to: "new-a".into(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    let after =
        basecraft_io::newick::parse_trees(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(after.len(), before.len());
    for (a, b) in after.iter().zip(&before) {
        assert_eq!((&a.name, a.rooted), (&b.name, b.rooted));
    }
    assert_eq!(
        &after[1..],
        &before[1..],
        "untouched member topology, lengths, labels or comments changed"
    );
    assert!(after[0].nodes.iter().any(|n| n.name == "new-a"));
    session.undo(doc).unwrap();
    session.save(doc, None).unwrap();
    assert_eq!(
        basecraft_io::newick::parse_trees(&std::fs::read_to_string(&file).unwrap()).unwrap(),
        before
    );
}

#[test]
fn interrupted_tree_metadata_save_is_explicit_and_never_reuses_old_semantics() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.nwk");
    std::fs::write(&original, "((a,b)90,c);\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&original).unwrap();
    session
        .apply(
            doc,
            Op::TreeRenameTip {
                from: "a".into(),
                to: "new-a".into(),
            },
        )
        .unwrap();
    let target = dir.path().join("partial.nwk");
    std::fs::create_dir(files::tree_sidecar(&target)).unwrap();
    let error = session.save(doc, Some(&target)).unwrap_err().to_string();
    // Every file is staged first; the tree replaced, its sidecar could not be.
    assert!(error.contains("sidecar incomplete"), "{error}");
    assert!(target.is_file());
    assert!(
        files::load_tree(&target)
            .unwrap()
            .metadata_warning
            .is_some()
    );
    assert_eq!(
        std::fs::read_to_string(&original).unwrap(),
        "((a,b)90,c);\n"
    );
    assert!(session.tree(doc).unwrap().to_newick().contains("new-a"));
}
