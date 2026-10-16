//! Independent storage review probes; production source is unchanged.
use basecraft_ops::{Session, files, provenance};
use basecraft_plugin_host::{
    FileJob, delimitation_pipeline::AlignmentJob, file_tool::AlignmentInput,
};
use serde_json::json;
use std::path::{Path, PathBuf};

fn plugin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/delimitation")
}

fn setup_tree(session: &mut Session, source: &Path, output: &Path) {
    let doc = session.open(source).unwrap();
    let job =
        FileJob::from_tree(session, doc, (&plugin(), "workspace_tree"), output.into()).unwrap();
    let result = job.run().unwrap();
    job.record(session, doc, &result).unwrap();
}

#[test]
fn nexus_source_workspace_can_reuse_its_display_tree() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.nex");
    std::fs::write(
        &source,
        "#NEXUS\nBEGIN TREES;\nTREE t = ((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\nEND;\n",
    )
    .unwrap();
    let canonical = root.path().join("species.spart");
    let mut session = Session::recoverable(root.path()).unwrap();
    setup_tree(&mut session, &source, &canonical);
    let doc = session.open(&canonical).unwrap();
    let display = basecraft_ops::delimitation::checked_display_tree(session.delimitation(doc))
        .unwrap()
        .unwrap();
    // The controller parses this checked display buffer as Newick on reopen.
    let bytes = std::fs::read_to_string(&display).unwrap();
    let parsed = basecraft_io::newick::parse_trees(&bytes);
    assert!(
        parsed.is_ok(),
        "checked display must be usable Newick: {display:?}: {parsed:?}"
    );
    let mut job = FileJob::from_file(
        &session,
        doc,
        &display,
        (&plugin(), "gmyc_single"),
        root.path().join("fit.json"),
    )
    .unwrap();
    job.params.insert("rooted".into(), json!(true));
    assert!(
        job.run().is_ok(),
        "workspace must reuse its accepted normalized tree"
    );
}

#[test]
fn alignment_gmyc_export_preserves_display_and_declared_inputs() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.afa");
    std::fs::write(
        &source,
        format!(
            ">a\n{}\n>b\n{}\n>c\n{}{}\n>d\n{}{}\n>e\n{}{}\n",
            "A".repeat(100),
            "A".repeat(100),
            "A".repeat(90),
            "C".repeat(10),
            "A".repeat(80),
            "G".repeat(20),
            "A".repeat(70),
            "T".repeat(30)
        ),
    )
    .unwrap();
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&source).unwrap();
    let mut preparation = FileJob::from_alignment(
        &session,
        doc,
        (&plugin(), "haplotypes"),
        AlignmentInput::Aligned,
        root.join("prepare.json"),
    )
    .unwrap();
    preparation.params.insert("model".into(), json!("p"));
    let outcome = AlignmentJob {
        preparation,
        unique_haplotypes: true,
        fit_method: "gmyc_single".into(),
        output: root.join("species.spart"),
    }
    .run();
    outcome.record(&mut session, doc).unwrap();
    let canonical = root.join("species.spart");
    let package = parent.path().join("package");
    provenance::package(&canonical, &package).unwrap();
    drop(session);
    std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
    let moved = parent.path().join("moved-package");
    std::fs::rename(&package, &moved).unwrap();
    let exported = moved.join("species.spart");
    assert!(provenance::verify(&exported).unwrap().is_empty());
    let work = files::load_delimitation(&exported).unwrap();
    let display = basecraft_ops::delimitation::checked_display_tree(&work)
        .unwrap()
        .unwrap();
    assert!(
        display.is_file(),
        "exported display is unavailable: {display:?}"
    );
    for hypothesis in &work.hypotheses {
        for (path, hash) in &hypothesis.provenance.inputs {
            assert_eq!(
                &basecraft_io::hash_file(Path::new(path)).unwrap(),
                hash,
                "exported input {path}"
            );
        }
    }
}

#[test]
fn preserved_tree_export_relocates_original_display_reference() {
    use basecraft_ops::input_evidence::{SavePolicy, conflicts, resolve};
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let canonical = root.join("species.spart");
    let mut session = Session::recoverable(&root).unwrap();
    setup_tree(&mut session, &source, &canonical);
    resolve(&conflicts(&source).unwrap(), SavePolicy::PreserveCopy).unwrap();
    std::fs::write(&source, "(changed:1,input:1);\n").unwrap();
    let original = files::load_delimitation(&canonical).unwrap();
    assert!(
        basecraft_ops::delimitation::checked_display_tree(&original)
            .unwrap()
            .unwrap()
            .is_file()
    );
    let package = parent.path().join("package");
    provenance::package(&canonical, &package).unwrap();
    drop(session);
    std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
    let exported = package.join("species.spart");
    assert!(provenance::verify(&exported).unwrap().is_empty());
    let work = files::load_delimitation(&exported).unwrap();
    let display = basecraft_ops::delimitation::checked_display_tree(&work);
    assert!(
        display.is_ok(),
        "complete export lost preserved display: {display:?}"
    );
    assert!(display.unwrap().unwrap().is_file());
}

#[test]
fn fitted_workspace_survives_folder_move_then_save_and_export() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.nwk");
    std::fs::write(&source, "((a:0.1,b:0.1):0.9,(c:0.2,d:0.2):0.8);\n").unwrap();
    let canonical = root.join("species.spart");
    let mut session = Session::recoverable(&root).unwrap();
    setup_tree(&mut session, &source, &canonical);
    let doc = session.open(&canonical).unwrap();
    let mut job = FileJob::from_file(
        &session,
        doc,
        &source,
        (&plugin(), "gmyc_single"),
        root.join("fit.json"),
    )
    .unwrap();
    job.params.insert("rooted".into(), json!(true));
    let result = job.run().unwrap();
    job.record(&mut session, doc, &result).unwrap();
    basecraft_ops::delimitation::add_analysis_result(&mut session, doc, &job.output).unwrap();
    session.save(doc, None).unwrap();
    drop(job);
    drop(session);
    let moved = parent.path().join("moved");
    std::fs::rename(&root, &moved).unwrap();
    let canonical = moved.join("species.spart");
    let mut session = Session::recoverable(&moved).unwrap();
    let doc = session.open(&canonical).unwrap();
    session.save(doc, None).unwrap();
    assert!(provenance::verify(&canonical).unwrap().is_empty());
    provenance::package(&canonical, &parent.path().join("package")).unwrap();
}

#[test]
fn preserved_alignment_remains_usable_by_workspace_default_input() {
    use basecraft_ops::input_evidence::{SavePolicy, conflicts, resolve};
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.afa");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAC\n>c\nAACC\n>d\nCCCC\n").unwrap();
    let canonical = root.path().join("species.spart");
    let mut session = Session::recoverable(root.path()).unwrap();
    let input = session.open(&source).unwrap();
    let setup = FileJob::from_alignment(
        &session,
        input,
        (&plugin(), "workspace_alignment"),
        AlignmentInput::Aligned,
        canonical.clone(),
    )
    .unwrap();
    let result = setup.run().unwrap();
    setup.record(&mut session, input, &result).unwrap();
    resolve(&conflicts(&source).unwrap(), SavePolicy::PreserveCopy).unwrap();
    std::fs::write(&source, ">a\nTTTT\n>b\nTTTC\n>c\nTTCC\n>d\nCCCC\n").unwrap();
    assert!(provenance::verify(&canonical).unwrap().is_empty());
    let doc = session.open(&canonical).unwrap();
    let work = session.delimitation(doc);
    // Mirrors open_file's linked_alignment default and run_analysis's hash gate.
    let linked = &work.exchange_fields["basecraft_alignment_snapshot"];
    let expected = &work.exchange_fields["basecraft_alignment_hash"];
    let job = FileJob::from_file(
        &session,
        doc,
        Path::new(linked),
        (&plugin(), "haplotypes"),
        root.path().join("prep.json"),
    )
    .unwrap();
    assert_eq!(
        &job.input_key.1, expected,
        "GUI default must follow the verified preserved alignment, as checked_display_tree does for trees"
    );
}

#[test]
fn combine_complete_export_includes_legacy_sidecar_evidence() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let text = include_str!("../../../testdata/fixtures/delimitation/manual.spart");
    let source = root.join("legacy.spart");
    let target = root.join("target.spart");
    std::fs::write(&source, text).unwrap();
    std::fs::write(&target, text).unwrap();
    let mut legacy = basecraft_io::spart::parse_text(text).unwrap();
    legacy.exchange_fields.insert(
        "authored_metadata".into(),
        "Distinct original sidecar context".into(),
    );
    legacy.hypotheses[0].provenance.parameters.insert(
        "reviewed_method_context".into(),
        json!("Sidecar-only scientific context"),
    );
    let mut bytes = Vec::new();
    files::write_delimitation_sidecar(
        &legacy,
        &basecraft_io::hash_file(&source).unwrap(),
        &mut bytes,
    )
    .unwrap();
    std::fs::write(files::tree_sidecar(&source), &bytes).unwrap();
    let side_hash = basecraft_io::hash_bytes(&bytes);
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&target).unwrap();
    basecraft_ops::delimitation::combine(&mut session, doc, &[source]).unwrap();
    let combined = root.join("combined.spart");
    session.save(doc, Some(&combined)).unwrap();
    let package = parent.path().join("package");
    provenance::package(&combined, &package).unwrap();
    drop(session);
    std::fs::rename(&root, parent.path().join("unavailable")).unwrap();
    let exported = package.join("combined.spart");
    assert!(provenance::verify(&exported).unwrap().is_empty());
    let artifacts = provenance::companion(&exported).join("artifacts");
    assert!(
        std::fs::read_dir(&artifacts)
            .unwrap()
            .any(|entry| basecraft_io::hash_file(&entry.unwrap().path()).unwrap() == side_hash),
        "complete export omitted the legacy sidecar whose hash Combine declares: {side_hash}"
    );
}

#[test]
#[cfg(unix)]
fn species_partial_save_retains_recoverable_edits_and_retries() {
    use basecraft_protocol::{Op, delimitation::DelimitationEdit};
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let canonical = root.path().join("species.spart");
    std::fs::write(
        &canonical,
        include_str!("../../../testdata/fixtures/delimitation/manual.spart"),
    )
    .unwrap();
    let mut session = Session::recoverable(root.path()).unwrap();
    let doc = session.open(&canonical).unwrap();
    session
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::Derive {
                    source: "partition-1".into(),
                    id: "editable".into(),
                    name: "Original".into(),
                }),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    session
        .apply(
            doc,
            Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::Rename {
                    hypothesis: "editable".into(),
                    name: "Pending edit".into(),
                }),
            },
        )
        .unwrap();
    let sidecar = files::tree_sidecar(&canonical);
    let directory = sidecar.parent().unwrap();
    let permissions = std::fs::metadata(directory).unwrap().permissions();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = session.save(doc, None);
    std::fs::set_permissions(directory, permissions).unwrap();
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("data saved") && error.contains("incomplete"),
        "{error}"
    );
    assert!(session.recovery_root().join("pending-save.json").is_file());
    let recovered = root.path().join("recovered");
    basecraft_ops::replay::recover(session.recovery_root(), &recovered).unwrap();
    let work = files::load_delimitation(&recovered.join("species.spart")).unwrap();
    assert_eq!(work.hypotheses.last().unwrap().name, "Pending edit");
    session.save(doc, None).unwrap();
    assert_eq!(
        files::load_delimitation(&canonical)
            .unwrap()
            .hypotheses
            .last()
            .unwrap()
            .name,
        "Pending edit"
    );
    assert!(provenance::verify(&canonical).unwrap().is_empty());
}
