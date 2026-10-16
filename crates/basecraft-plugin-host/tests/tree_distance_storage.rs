//! Real engines and real directory listings for the bounded tree/matrix rollout.
use std::path::Path;

use basecraft_ops::{Session, analysis_store::ArtifactRun, input_evidence, provenance};
use basecraft_plugin_host::{FileJob, file_tool::AlignmentInput};
use basecraft_protocol::Op;
use serde_json::{Value, json};

const ALIGNMENT: &str = ">a\nACGTACGTACGTACGTACGTACGT\n>b\nACGTACGTTCGTACGTACGTACGT\n>c\nTCGTACGTACGTACGTACGTACGA\n>d\nTCGTACGTTCGTACGTACGTACGA\n";

fn names(root: &Path) -> Vec<String> {
    let mut out: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn job(
    session: &Session,
    doc: basecraft_ops::DocId,
    plugin: &str,
    operation: &str,
    output: &Path,
) -> FileJob {
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    FileJob::from_alignment(
        session,
        doc,
        (&plugins.join(plugin), operation),
        AlignmentInput::Aligned,
        output.into(),
    )
    .unwrap()
}

fn listing(root: &Path, run: &Path, label: &str) {
    println!(
        "{label}: visible={:?}; hidden={:?}",
        names(root),
        names(run)
    );
}

#[test]
fn matrix_reference_generated_failure_move_and_export_use_the_common_store() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("working");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.afa");
    std::fs::write(&source, ALIGNMENT).unwrap();
    let mut session = Session::new(&root);
    let doc = session.open(&source).unwrap();
    let output = root.join("matrix.tsv");
    let clean = job(&session, doc, "distance", "matrix", &output);
    let result = clean.run().unwrap();
    clean.record(&mut session, doc, &result).unwrap();
    let run = ArtifactRun::for_result(&output)
        .unwrap()
        .expect("matrix must be bound to hidden storage");
    assert_eq!(names(&root), [".basecraft", "matrix.tsv", "source.afa"]);
    assert!(run.join("matrix.tsv.distances.json").is_file());
    assert!(run.join("matrix.tsv.run.json").is_file());
    assert!(!run.join("submitted.fasta").exists());
    assert!(!run.join("preserved-input").exists());
    let input = input_evidence::read_input(&run).unwrap();
    assert_eq!(input.state, "referenced");
    assert_eq!(
        input.source.unwrap().hash,
        basecraft_io::hash_file(&source).unwrap()
    );
    assert_eq!(
        clean.input_io().unwrap()["persistent_original_copy_bytes"],
        0
    );
    assert!(provenance::verify(&output).unwrap().is_empty());
    listing(&root, &run, "matrix-clean");

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
    let dirty_output = root.join("dirty.tsv");
    let dirty = job(&session, doc, "distance", "matrix", &dirty_output);
    let result = dirty.run().unwrap();
    dirty.record(&mut session, doc, &result).unwrap();
    let dirty_run = ArtifactRun::for_result(&dirty_output).unwrap().unwrap();
    let input = input_evidence::read_input(&dirty_run).unwrap();
    assert_eq!(input.state, "generated");
    assert!(input.source.is_none());
    assert_eq!(
        input.submitted_hash,
        basecraft_io::hash_file(&dirty_run.join("submitted.fasta")).unwrap()
    );
    assert!(
        std::fs::read_to_string(dirty_run.join("submitted.fasta"))
            .unwrap()
            .starts_with(">a\nTCGT")
    );
    assert_eq!(std::fs::read_to_string(&source).unwrap(), ALIGNMENT);
    listing(&root, &dirty_run, "matrix-dirty");

    let failed_output = root.join("failed.tsv");
    let mut failed = job(&session, doc, "distance", "matrix", &failed_output);
    failed.params.insert("model".into(), json!("unsupported"));
    let outcome = failed.run();
    assert!(outcome.is_err());
    failed
        .record_terminal(&mut session, doc, &outcome, 0.0)
        .unwrap();
    assert!(!failed_output.exists());
    let runs = root.join(".basecraft/analyses-v1/runs");
    let failed_run = std::fs::read_dir(&runs)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| read(&p.join("execution.json"))["status"] == "failed")
        .unwrap();
    let execution = read(&failed_run.join("execution.json"));
    assert_eq!(execution["effective_parameters"]["model"], "unsupported");
    assert!(execution["diagnostics"].as_str().unwrap().contains("Model"));
    listing(&root, &failed_run, "matrix-failed");
    assert_eq!(
        names(&root),
        [".basecraft", "dirty.tsv", "matrix.tsv", "source.afa"]
    );
    session.flush().unwrap();
    assert!(basecraft_ops::replay::replay(&root).unwrap().ok());
    drop(session);

    let moved = parent.path().join("moved");
    std::fs::rename(&root, &moved).unwrap();
    let output = moved.join("matrix.tsv");
    assert!(provenance::verify(&output).unwrap().is_empty());
    let package = parent.path().join("export");
    provenance::package(&output, &package).unwrap();
    std::fs::rename(&moved, parent.path().join("unavailable")).unwrap();
    assert!(
        provenance::verify(&package.join("matrix.tsv"))
            .unwrap()
            .is_empty()
    );
    println!("matrix-export: {:?}", names(&package));
}

#[test]
fn ordinary_tree_and_forest_routes_keep_only_scientific_results_visible() {
    for (plugin, method, alignment, expected) in [
        ("distance", "sorted", ALIGNMENT, vec!["result.nwk"]),
        ("distance", "upgma", ALIGNMENT, vec!["result.nwk"]),
        (
            "distance",
            "njstar",
            ">a\nAAA---\n>b\nAAT---\n>c\n---CCC\n>d\n---CCT\n",
            vec![
                "result.component-1.nwk",
                "result.component-2.nwk",
                "result.nwk",
            ],
        ),
        ("fasttree", "", ALIGNMENT, vec!["result.nwk"]),
        ("iqtree", "", ALIGNMENT, vec!["result.nwk"]),
    ] {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path();
        let source = root.join("source.afa");
        std::fs::write(&source, alignment).unwrap();
        let mut session = Session::new(root);
        let doc = session.open(&source).unwrap();
        let output = root.join("result.nwk");
        let mut job = job(&session, doc, plugin, "tree", &output);
        job.params.insert("threads".into(), json!(1));
        job.params.insert(
            "model".into(),
            json!(if plugin == "iqtree" { "JC" } else { "p" }),
        );
        job.params.insert("seed".into(), json!(71));
        if !method.is_empty() {
            job.params.insert("method".into(), json!(method));
        }
        let result = job.run().unwrap();
        job.record(&mut session, doc, &result).unwrap();
        let run = ArtifactRun::for_result(&output).unwrap().unwrap();
        assert!(!run.join("submitted.fasta").exists());
        assert!(!run.join("preserved-input").exists());
        let execution = read(&run.join("execution.json"));
        assert_eq!(execution["status"], "completed");
        assert!(
            execution["tool"]["version"]
                .as_str()
                .is_some_and(|s| !s.is_empty() && s != "unknown")
        );
        let mut visible = vec![".basecraft", "source.afa"];
        visible.extend(expected.iter().copied());
        visible.sort_unstable();
        assert_eq!(names(root), visible);
        for name in expected {
            let tree = root.join(name);
            assert_eq!(ArtifactRun::for_result(&tree).unwrap().unwrap(), run);
            let id = session.open(&tree).unwrap();
            let link = basecraft_ops::links::AlignmentLink::read(&tree).unwrap();
            assert_eq!(link.source, source);
            assert!(link.checked_snapshot(&tree, session.tree(id)).is_ok());
            session.save(id, None).unwrap();
            assert!(basecraft_ops::files::tree_sidecar(&tree).starts_with(&run));
        }
        assert_eq!(names(root), visible);
        listing(root, &run, &format!("{plugin}-{method}"));
        if plugin == "iqtree" {
            assert!(run.join("result.nwk.iqtree").is_file());
            assert!(run.join("result.nwk.log").is_file());
            assert_eq!(read(&run.join("result.nwk.run.json"))["seed"], 71);
        }
    }
}
