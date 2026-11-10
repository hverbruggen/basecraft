//! Actual original-writer byte oracles, captured before constructor replacement.
use std::path::{Path, PathBuf};

use basecraft_ops::{Session, analysis_store::ArtifactRun, input_evidence};
use basecraft_protocol::Actor;
use serde_json::{Value, json};

fn capture(name: &str, run: &Path, expected: Value) {
    let actual = std::fs::read(run.join("run.json")).unwrap();
    let mut original = serde_json::to_vec_pretty(&expected).unwrap();
    original.push(b'\n');
    assert_eq!(actual, original, "{name}");
    if let Some(root) = std::env::var_os("BASECRAFT_RECORD_CAPTURE") {
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(Path::new(&root).join(format!("{name}.json")), actual).unwrap();
        std::fs::write(
            Path::new(&root).join(format!("{name}.original.json")),
            original,
        )
        .unwrap();
    }
}

#[test]
fn original_reference_and_generated_run_writers() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.fasta");
    std::fs::write(&source, b">sample\nACGT\n").unwrap();
    let run = input_evidence::reference_input_with_dependencies(
        &root.path().join("ünicode result.txt"),
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        &[],
        "run record fixture",
    )
    .unwrap();
    capture(
        "run-referenced",
        run.directory(),
        json!({"schema_version":1,"id":run.id(),"result":"ünicode result.txt","status":"referenced"}),
    );
    let generated = ArtifactRun::create(&root.path().join("result.txt"), "run-generated").unwrap();
    input_evidence::link_generated_input(
        &generated,
        &[],
        b"generated input",
        "identity-v1",
        "Document snapshot",
    )
    .unwrap();
    capture(
        "run-generated",
        generated.directory(),
        json!({"schema_version":1,"id":generated.id(),"result":"Document snapshot","status":"prepared"}),
    );
}

#[cfg(unix)]
#[test]
fn original_reference_null_result() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.fasta");
    std::fs::write(&source, b">sample\nACGT\n").unwrap();
    let anchor = root.path().join(OsString::from_vec(vec![0xff]));
    let run = input_evidence::reference_input_with_dependencies(
        &anchor,
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        &[],
        "run record fixture",
    )
    .unwrap();
    capture(
        "run-null",
        run.directory(),
        json!({"schema_version":1,"id":run.id(),"result":null,"status":"referenced"}),
    );
}

#[test]
fn original_delimitation_save_run_writer() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("input.spart");
    std::fs::write(
        &target,
        include_bytes!("../../../testdata/fixtures/delimitation/manual.spart"),
    )
    .unwrap();
    let mut session = Session::new(root.path());
    let doc = session.open(&target).unwrap();
    let mut view = session.delimitation(doc).view.clone();
    view.visible.reverse();
    session
        .apply(
            doc,
            basecraft_protocol::Op::DelimitationEdit {
                edit: Box::new(
                    basecraft_protocol::delimitation::DelimitationEdit::SetView { view },
                ),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    let run = ArtifactRun::for_result(&target).unwrap().unwrap();
    // Identity comes from the public binding/directory, not the run record.
    let id = run.file_name().unwrap().to_str().unwrap();
    capture(
        "run-saved",
        &run,
        json!({"schema_version":1,"id":id,"result":"input.spart","status":"saved"}),
    );
}

#[test]
fn original_referenced_publication_run_writer() {
    for with_outputs in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.fasta");
        std::fs::write(&source, b">sample\nACGT\n").unwrap();
        let target = root.path().join("result.txt");
        let payload = b"immutable output\n";
        std::fs::write(&target, payload).unwrap();
        let run = input_evidence::reference_input_with_dependencies(
            &target,
            &source,
            &basecraft_io::hash_file(&source).unwrap(),
            &[],
            "run record fixture",
        )
        .unwrap();
        let mut session = Session::new(root.path());
        let doc = session.open(&source).unwrap();
        let outputs: Vec<PathBuf> = if with_outputs {
            vec![target.clone()]
        } else {
            vec![]
        };
        session
            .log_tool_run(
                doc,
                Actor::Script {
                    client: "record fixture".into(),
                },
                ("fixture", "count"),
                json!({}),
                &[],
                &outputs,
            )
            .unwrap();
        let snapshot = session.document_record(doc).unwrap();
        session
            .publish_referenced_analysis(doc, &target, &outputs, &snapshot, &run)
            .unwrap();
        // Independently derive every output field from the accepted input bytes,
        // destination, and allocated run; do not borrow publication/run fields.
        let artifacts = if with_outputs {
            json!([{"id":format!("{}:output:0",run.id()),"logical_name":"result.txt","hash":basecraft_io::hash_bytes(payload),"size":payload.len(),"retained":null}])
        } else {
            json!([])
        };
        capture(
            if with_outputs {
                "run-completed"
            } else {
                "run-completed-empty"
            },
            run.directory(),
            json!({"schema_version":1,"id":run.id(),"result":"result.txt","status":"completed","outputs":artifacts}),
        );
    }
}
