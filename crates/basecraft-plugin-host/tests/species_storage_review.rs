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
