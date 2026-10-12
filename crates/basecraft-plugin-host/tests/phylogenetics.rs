//! Real local/pixi tool runs through the same boundary as the GUI.
use basecraft_ops::Session;
use basecraft_plugin_host::{FileJob, file_tool::AlignmentInput, process::python_interpreter};
use basecraft_protocol::Op;
use std::path::Path;

fn available(name: &str) -> bool {
    std::process::Command::new(python_interpreter()).args(["-c", &format!(
        "import shutil,sys,pathlib; sys.exit(0 if (pathlib.Path(sys.prefix)/'bin'/'{name}').exists() or shutil.which('{name}') else 1)")])
        .status().is_ok_and(|s|s.success())
}

#[test]
fn trimal_and_iqtree_preserve_identifiers_and_provenance() {
    if !available("trimal") || !available("iqtree") {
        eprintln!("Skipping: install pixi bio tools");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let input = root.join("input.afa");
    std::fs::write(&input, ">a:one\nACGTACGTACGT--ACGTACGTACGT\n>b\nACGTACGTTCGT--ACGTACGTACGT\n>c\nTCGTACGTACGT--ACGTACGTACGA\n>d\nTCGTACGTTCGT--ACGTACGTACGA\n").unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let trimmed = root.join("trimmed.afa");
    let job = FileJob::from_alignment(
        &session,
        doc,
        (&plugins.join("trimal"), "trim"),
        AlignmentInput::Aligned,
        trimmed.clone(),
    )
    .unwrap();
    let result = job.run().unwrap();
    job.record(&mut session, doc, &result).unwrap();
    let out = session.open(&trimmed).unwrap();
    assert_eq!(session.alignment(out).width(), 24);
    assert_eq!(session.alignment(out).row(0).name, "a:one");
    session
        .apply(
            doc,
            Op::AlnReplace {
                row: 0,
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    let tree = root.join("tree.nwk");
    let mut job = FileJob::from_alignment(
        &session,
        doc,
        (&plugins.join("iqtree"), "tree"),
        AlignmentInput::Aligned,
        tree.clone(),
    )
    .unwrap();
    job.params.insert("model".into(), serde_json::json!("JC"));
    job.params.insert("threads".into(), serde_json::json!(1));
    let result = job.run().unwrap();
    job.record(&mut session, doc, &result).unwrap();
    let tree_doc = session.open(&tree).unwrap();
    assert!(session.tree(tree_doc).find_tip("a:one").is_some());
    assert!(result.outputs.iter().any(|p| p.ends_with(".iqtree")));
    session.flush().unwrap();
    assert!(basecraft_ops::replay::replay(root).unwrap().ok());
}
