use basecraft_ops::{Session, files};
use basecraft_protocol::Op;

#[test]
fn same_path_replacement_cannot_reuse_tree_metadata() {
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
                kind: Default::default(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    std::fs::write(&path, "((c,b)10,a);\n").unwrap();
    assert!(
        files::load_tree(&path).is_err(),
        "stale metadata was accepted"
    );
}

#[test]
fn undone_import_is_absent_from_methods() {
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
fn legacy_metadata_opens_with_warning_and_survives_explicit_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.nwk");
    std::fs::write(&path, "((a,b)90,c);\n").unwrap();
    let side = files::tree_sidecar(&path);
    let legacy =
        br#"{"supports":[{"name":"Do not apply","source":"old"}],"annotations":{},"style":{}}"#;
    // Use the complete default style serializer to keep this a real old format.
    let mut value = serde_json::to_value(basecraft_protocol::TreeStyle::default()).unwrap();
    let style = value.take();
    let bytes = serde_json::to_vec(&serde_json::json!({"supports":[{"name":"Do not apply","source":"old"}],"annotations":{},"style":style})).unwrap();
    let _ = legacy;
    std::fs::write(&side, &bytes).unwrap();
    let opened = files::load_tree(&path).unwrap();
    assert!(opened.metadata_warning.is_some());
    assert_eq!(opened.doc.supports()[0].name, "Support");
    assert_eq!(std::fs::read(&side).unwrap(), bytes);
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    assert!(
        session
            .document_record(doc)
            .unwrap()
            .report()
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("Unverified legacy"))
    );
    session
        .apply(doc, Op::TreeLadderize { small_first: true })
        .unwrap();
    session.save(doc, None).unwrap();
    assert!(
        std::fs::read_dir(dir.path()).unwrap().flatten().any(|e| e
            .file_name()
            .to_string_lossy()
            .contains("unverified-")
            && std::fs::read(e.path()).ok().as_deref() == Some(bytes.as_slice()))
    );
    assert!(files::load_tree(&path).unwrap().metadata_warning.is_none());
}

#[test]
fn hidden_binding_errors_never_fall_back_to_adjacent_metadata() {
    use basecraft_ops::analysis_store::ArtifactRun;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forest.nwk");
    std::fs::write(&path, "((a,b)90,c);\n(a,b,c);\n").unwrap();
    let tree = files::load_tree(&path).unwrap().doc;
    let mut metadata = Vec::new();
    files::write_tree_sidecar_at(
        &tree,
        &basecraft_io::hash_file(&path).unwrap(),
        &mut metadata,
    )
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
    let binding = std::fs::read_dir(dir.path().join(".basecraft/analyses-v1/bindings"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original = std::fs::read(&binding).unwrap();
    for broken in [b"{".as_slice(), br#"{"schema_version":42}"#.as_slice()] {
        std::fs::write(&binding, broken).unwrap();
        assert!(files::load_tree(&path).is_err());
    }
    std::fs::write(&binding, original).unwrap();
    // The first tree still matches: the full forest binding must detect this.
    std::fs::write(&path, "((a,b)90,c);\n(x,y,z);\n").unwrap();
    assert!(files::load_tree(&path).is_err());
    assert!(
        ArtifactRun::for_result(&path).unwrap().is_some(),
        "writer lookup must remain available"
    );
}

#[test]
fn imported_support_partial_removal_undo_save_reopen_and_redo() {
    use basecraft_ops::provenance;
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
    let input =
        basecraft_ops::input_evidence::read_input(&provenance::bundle_for_result(&target)).unwrap();
    assert!(input.extra["upstream"].as_array().unwrap().is_empty());
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
        provenance::verify(&target).unwrap().is_empty(),
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
            qualifiers: Default::default(),
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
            session.generation(doc),
            session.revision(doc),
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
}

#[test]
fn generic_no_output_activity_does_not_own_unrelated_manual_edits() {
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
    use basecraft_ops::provenance;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("gene.fasta");
    let result = dir.path().join("extract.gb");
    std::fs::write(&source, ">gene\nACGTACGT\n").unwrap();
    let mut session = Session::recoverable_with_paths(dir.path(), &paths(dir.path())).unwrap();
    let doc = session.open(&source).unwrap();
    session
        .extract(doc, (1, 7), &[(1, 3), (5, 7)], true, &result)
        .unwrap();
    let publication = provenance::inspect(&result).unwrap();
    assert!(publication.inputs[0].retained.is_none());
    assert!(publication.inputs[0].reference.is_some());
    let issues = provenance::verify(&result).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    let input =
        basecraft_ops::input_evidence::read_input(&provenance::bundle_for_result(&result)).unwrap();
    assert_eq!(
        input.extra["extraction"]["selection"]["segments"],
        serde_json::json!([[1, 3], [5, 7]])
    );
    let package = dir.path().join("package");
    provenance::package(&result, &package).unwrap();
    std::fs::write(&source, ">gene\nTTTTTTTT\n").unwrap();
    assert!(!provenance::verify(&result).unwrap().is_empty());
    assert!(
        provenance::verify(&package.join("extract.gb"))
            .unwrap()
            .is_empty()
    );
}
