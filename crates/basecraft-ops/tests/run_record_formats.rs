//! Actual original-writer byte oracles, captured before constructor replacement.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use std::path::{Path, PathBuf};

use basecraft_ops::{Session, analysis_store::ArtifactRun, input_evidence};
use basecraft_protocol::Actor;
use serde_json::{Value, json};

fn capture(name: &str, run: &Path, expected: &Value) {
    let actual = std::fs::read(run.join("run.json")).unwrap();
    let mut original = serde_json::to_vec_pretty(expected).unwrap();
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
    crate::isolated_profile::isolate!();
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
        &json!({"schema_version":1,"id":run.id(),"result":"ünicode result.txt","status":"referenced"}),
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
        &json!({"schema_version":1,"id":generated.id(),"result":"Document snapshot","status":"prepared"}),
    );
}

#[cfg(unix)]
#[test]
fn original_reference_null_result() {
    crate::isolated_profile::isolate!();
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
        &json!({"schema_version":1,"id":run.id(),"result":null,"status":"referenced"}),
    );
}

#[test]
fn original_delimitation_save_run_writer() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("input.spart");
    std::fs::write(
        &target,
        include_bytes!("../../../testdata/fixtures/delimitation/manual.spart"),
    )
    .unwrap();
    let mut session = Session::new(root.path());
    let doc = session.open(&target).unwrap();
    let target = root.path().join("saved.spart");
    session.save(doc, Some(&target)).unwrap();
    let run = ArtifactRun::for_result(&target).unwrap().unwrap();
    // Identity comes from the public binding/directory, not the run record.
    let id = run.file_name().unwrap().to_str().unwrap();
    capture(
        "run-saved",
        &run,
        &json!({"schema_version":1,"id":id,"result":"saved.spart","status":"saved"}),
    );
}

#[test]
fn original_referenced_publication_run_writer() {
    crate::isolated_profile::isolate!();
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
            &json!({"schema_version":1,"id":run.id(),"result":"result.txt","status":"completed","outputs":artifacts}),
        );
    }
}

#[test]
fn run_examples_roundtrip_and_preserve_extensions() {
    crate::isolated_profile::isolate!();
    use basecraft_protocol::records::{AnalysisRun, AnalysisRunStatus};
    let fixture_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/records-v1");
    for name in [
        "run-extracted.json",
        "run-referenced.json",
        "run-null.json",
        "run-generated.json",
        "run-saved.json",
        "run-completed.json",
        "run-completed-empty.json",
        "run-prepared.json",
        "run-prepared-no-export.json",
    ] {
        let bytes = std::fs::read(fixture_root.join(name)).unwrap();
        let parsed: AnalysisRun = serde_json::from_slice(&bytes).unwrap();
        let value = serde_json::to_value(parsed).unwrap();
        let mut actual = serde_json::to_vec_pretty(&value).unwrap();
        actual.push(b'\n');
        assert_eq!(actual, bytes, "{name}");
        let mut extended = value;
        extended["future"] = json!({"z":[null,{"z":3,"a":true}],"a":"kept"});
        extended["status"] = json!("future-status-v2");
        if let Some(outputs) = extended.get_mut("outputs").and_then(Value::as_array_mut) {
            for output in outputs {
                output["future"] = json!({"nested":[null, "kept"]});
            }
        }
        let parsed: AnalysisRun = serde_json::from_value(extended.clone()).unwrap();
        assert_eq!(
            parsed.status,
            AnalysisRunStatus::Unknown("future-status-v2".into())
        );
        let actual = serde_json::to_value(parsed).unwrap();
        assert_eq!(actual, extended, "{name} extensions/null/omission");
        // Also pin sorted bytes, including nested extra maps.
        assert_eq!(
            serde_json::to_vec_pretty(&actual).unwrap(),
            serde_json::to_vec_pretty(&extended).unwrap()
        );
    }
    for (label, expected) in [
        ("referenced", AnalysisRunStatus::Referenced),
        ("prepared", AnalysisRunStatus::Prepared),
        ("saved", AnalysisRunStatus::Saved),
        ("completed", AnalysisRunStatus::Completed),
        ("", AnalysisRunStatus::Unknown(String::new())),
    ] {
        let state: AnalysisRunStatus = serde_json::from_value(json!(label)).unwrap();
        assert_eq!(state, expected);
        assert_eq!(serde_json::to_value(state).unwrap(), json!(label));
    }
}

#[test]
fn original_extraction_run_writer_inventory() {
    crate::isolated_profile::isolate!();
    // Captured while the sixth production constructor was still the original
    // literal, before the user expanded scope to include its substitution.
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.fasta");
    std::fs::write(&source, b">sample\nACGT\n").unwrap();
    let mut session = Session::new(root.path());
    let doc = session.open(&source).unwrap();
    let target = root.path().join("extracted.gb");
    session.extract(doc, (0, 4), &[], false, &target).unwrap();
    let run = ArtifactRun::for_result(&target).unwrap().unwrap();
    let id = run.file_name().unwrap().to_str().unwrap();
    capture(
        "run-extracted",
        &run,
        &json!({"schema_version":1,"id":id,"result":"extracted.gb","status":"saved"}),
    );
}
