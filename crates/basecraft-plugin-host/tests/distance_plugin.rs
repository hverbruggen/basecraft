//! Public plugin boundary, output provenance, and operation-log replay.
use basecraft_ops::Session;
use basecraft_plugin_host::{FileJob, file_tool::AlignmentInput};
use basecraft_protocol::Op;
#[test]
fn distance_outputs_are_logged_and_replay() {
    let Some(binary) = std::env::var_os("BASECRAFT_DISTANCE_TEST_BINARY") else {
        eprintln!("set BASECRAFT_DISTANCE_TEST_BINARY to exercise the distance CLI plugin");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = root.join("input.aln.fasta");
    std::fs::write(
        &input,
        ">a\nAAAAAAAC\n>b\nAAAAAACC\n>c\nCCAAAAAC\n>d\nCCAAAACC\n",
    )
    .unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/distance");
    std::fs::create_dir(root.join("nested")).unwrap();
    let output = root.join("nested/../tree.nwk");
    let mut job = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "tree"),
        AlignmentInput::Aligned,
        output.clone(),
    )
    .unwrap();
    job.params.insert(
        "executable".into(),
        serde_json::json!(std::path::PathBuf::from(binary)),
    );
    job.params.insert("model".into(), serde_json::json!("jc69"));
    job.params.insert("threads".into(), serde_json::json!(2));
    let result = job.run().unwrap();
    assert_eq!(result.outputs.len(), 4);
    job.record(&mut session, doc, &result).unwrap();
    let tree = session.open(&output).unwrap();
    assert_eq!(session.tree(tree).tip_count(), 4);
    let log = basecraft_ops::log::read_all(root).unwrap();
    let entry = log
        .iter()
        .find(|e| matches!(e.op, Op::ToolRun { .. }))
        .unwrap();
    assert_eq!(entry.result.hashes.len(), 5);
    if let Op::ToolRun {
        params, outputs, ..
    } = &entry.op
    {
        assert_eq!(params["model"], "jc69");
        assert_eq!(outputs.len(), 4);
    }
    let mut upgma = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "tree"),
        AlignmentInput::Aligned,
        root.join("upgma.nwk"),
    )
    .unwrap();
    upgma.params.clone_from(&job.params);
    upgma
        .params
        .insert("method".into(), serde_json::json!("upgma"));
    let result = upgma.run().unwrap();
    upgma.record(&mut session, doc, &result).unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            basecraft_ops::provenance::bundle_for_result(&root.join("upgma.nwk"))
                .join("upgma.nwk.run.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(metadata["method"], "upgma");
    let tree = session.open(&root.join("upgma.nwk")).unwrap();
    assert_eq!(session.tree(tree).tip_count(), 4);
    let replay = basecraft_ops::replay::replay(root).unwrap();
    assert!(replay.ok(), "{:?}", replay.problems);
}

#[test]
fn forest_members_share_one_alignment_snapshot() {
    let Some(binary) = std::env::var_os("BASECRAFT_DISTANCE_TEST_BINARY") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("forest.afa");
    std::fs::write(&input, ">a\nAAA---\n>b\nAAT---\n>c\n---CCC\n>d\n---CCT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&input).unwrap();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/distance");
    assert!(
        FileJob::from_alignment(
            &session,
            doc,
            (&plugin, "tree"),
            AlignmentInput::Aligned,
            input.clone()
        )
        .is_err()
    );
    let output = dir.path().join("forest.nwk");
    let mut job = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "tree"),
        AlignmentInput::Aligned,
        output.clone(),
    )
    .unwrap();
    job.params.insert(
        "executable".into(),
        serde_json::json!(std::path::PathBuf::from(binary)),
    );
    job.params.insert("model".into(), serde_json::json!("p"));
    let result = job.run().unwrap();
    job.record(&mut session, doc, &result).unwrap();
    let main = basecraft_ops::links::AlignmentLink::read(&output).unwrap();
    let mut components = 0;
    for path in &result.outputs {
        if path.contains(".component-")
            && std::path::Path::new(path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("nwk"))
        {
            components += 1;
            let path = std::path::Path::new(path);
            let tree = session.open(path).unwrap();
            let link = basecraft_ops::links::AlignmentLink::read(path).unwrap();
            assert_eq!(link.snapshot, main.snapshot);
            assert!(link.checked_snapshot(path, session.tree(tree)).is_ok());
        }
    }
    assert_eq!(components, 2);
    assert_eq!(
        result
            .outputs
            .iter()
            .filter(|p| p.ends_with(".alignment.afa"))
            .count(),
        0
    );
    session.flush().unwrap();
    assert!(basecraft_ops::replay::replay(dir.path()).unwrap().ok());
}

#[test]
fn bootstrap_provenance_success_and_withheld_support_replay() {
    let Some(binary) = std::env::var_os("BASECRAFT_DISTANCE_TEST_BINARY") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/distance");
    for (case, input_text) in [
        (
            "complete",
            ">a\nAAAAAAAAAACCAAAACCAA\n>b\nAAAAAAAAAACCAAAACCAA\n>c\nAAAAGGAAAACCAAAACCAA\n>d\nAAAAGGAAAACCAAAACCAA\n",
        ),
        ("gaps", ">a\nA---\n>b\nAAAA\n"),
    ] {
        let input = root.join(format!("{case}.afa"));
        std::fs::write(&input, input_text).unwrap();
        let mut session = Session::new(root);
        let doc = session.open(&input).unwrap();
        for method in ["sorted", "upgma"] {
            let output = root.join(format!("{case}-{method}.nwk"));
            let mut job = FileJob::from_alignment(
                &session,
                doc,
                (&plugin, "tree"),
                AlignmentInput::Aligned,
                output.clone(),
            )
            .unwrap();
            job.params.insert(
                "executable".into(),
                serde_json::json!(std::path::PathBuf::from(&binary)),
            );
            job.params.insert("model".into(), serde_json::json!("p"));
            job.params
                .insert("method".into(), serde_json::json!(method));
            job.params.insert("bootstrap".into(), serde_json::json!(64));
            job.params.insert("seed".into(), serde_json::json!(71));
            job.params.insert("threads".into(), serde_json::json!(2));
            let result = job.run().unwrap();
            job.record(&mut session, doc, &result).unwrap();
            let report: serde_json::Value = serde_json::from_slice(
                &std::fs::read(basecraft_ops::provenance::bundle_for_result(&output).join(
                    format!("{}.run.json", output.file_name().unwrap().to_string_lossy()),
                ))
                .unwrap(),
            )
            .unwrap();
            assert_eq!(report["bootstrap"]["requested"], 64);
            assert_eq!(report["bootstrap"]["seed"], 71);
            assert_eq!(report["bootstrap"]["method"], method);
            assert_eq!(report["bootstrap"]["support_withheld"], case == "gaps");
            if case == "gaps" {
                assert!(result.message.unwrap().contains("support withheld"));
            } else {
                assert_eq!(report["bootstrap"]["completed"], 64);
                let tree = session.open(&output).unwrap();
                let generated = session.tree(tree);
                assert_eq!(generated.supports()[0].name, "Bootstrap");
                assert_eq!(
                    generated.supports()[0].kind,
                    basecraft_protocol::SupportKind::Bootstrap
                );
                assert_eq!(generated.supports()[0].decimals(generated.style()), 0);
                let link = basecraft_ops::links::AlignmentLink::read(&output).unwrap();
                assert!(link.inferred);
                assert_eq!(link.source, input);
                assert!(link.checked_snapshot(&output, generated).is_ok());
                session.save(tree, None).unwrap();
                let reopened = basecraft_ops::files::load_tree(&output).unwrap().doc;
                assert_eq!(reopened.supports()[0].name, "Bootstrap");
                assert_eq!(
                    reopened.supports()[0].kind,
                    basecraft_protocol::SupportKind::Bootstrap
                );
            }
        }
        session.flush().unwrap();
    }
    let log = basecraft_ops::log::read_all(root).unwrap();
    assert_eq!(
        log.iter()
            .filter(|e| matches!(e.op, Op::ToolRun { .. }))
            .count(),
        4
    );
    for entry in log.iter().filter(|e| matches!(e.op, Op::ToolRun { .. })) {
        if let Op::ToolRun { params, .. } = &entry.op {
            assert_eq!(params["bootstrap"], 64);
            assert_eq!(params["seed"], 71);
        }
    }
    let replay = basecraft_ops::replay::replay(root).unwrap();
    assert!(replay.ok(), "{:?}", replay.problems);
}

#[test]
fn failed_native_publication_registers_structured_bootstrap_evidence_without_input_copies() {
    let binary = std::env::var_os("BASECRAFT_DISTANCE_TEST_BINARY")
        .expect("set matching distance test binary for combined host acceptance");
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input.afa");
    std::fs::write(&source, ">a\nAAAAAC\n>b\nAAAAAT\n>c\nACAAAC\n").unwrap();
    let output = root.path().join("failed.nwk");
    let mut session = Session::recoverable(root.path()).unwrap();
    let doc = session.open(&source).unwrap();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/distance");
    let mut job = FileJob::from_alignment(
        &session,
        doc,
        (&plugin, "tree"),
        AlignmentInput::Aligned,
        output.clone(),
    )
    .unwrap();
    let cache_directory = root.path().join("read-only");
    std::fs::create_dir(&cache_directory).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cache_directory, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let missing = cache_directory.join("cache.json");
    job.params.extend([
        (
            "executable".into(),
            serde_json::json!(std::path::PathBuf::from(binary)),
        ),
        ("model".into(), serde_json::json!("p")),
        ("method".into(), serde_json::json!("sorted")),
        ("bootstrap".into(), serde_json::json!(8)),
        ("threads".into(), serde_json::json!(1)),
        ("cache".into(), serde_json::json!(missing)),
    ]);
    let result = job.run();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cache_directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(result.is_err(), "{result:?}");
    job.record_terminal(&mut session, doc, &result, 0.0)
        .unwrap();
    assert!(!output.exists());
    let paths = basecraft_platform::paths::AppPaths::discover().unwrap();
    let run_id = job.params["job_id"].as_str().unwrap();
    let execution: serde_json::Value = serde_json::from_slice(
        &std::fs::read(paths.state.join("jobs").join(run_id).join("manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(execution["status"], "failed");
    let collection = root
        .path()
        .join(".basecraft/analyses-v1/runs")
        .join(run_id)
        .join(format!("bootstrap-{run_id}.nex"));
    assert!(collection.is_file());
    assert!(
        execution["outputs"]
            .get(collection.to_str().unwrap())
            .is_some(),
        "{execution}"
    );
    assert_eq!(execution["recovery_artifact"]["status"], "complete");
    assert_eq!(execution["recovery_artifact"]["trees"], 8);
    assert_eq!(job.input_io().unwrap()["persistent_original_copy_bytes"], 0);
    assert!(
        !collection
            .parent()
            .unwrap()
            .join("submitted.fasta")
            .exists()
    );
}
