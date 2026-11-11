use basecraft_ops::document_record::ReportExt;
use basecraft_ops::{blast, document_record::DocumentRecord, metadata};
use serde_json::json;
use std::path::Path;

fn golden(name: &str, bytes: &[u8]) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/records-v1")
        .join(name);
    if std::env::var_os("BASECRAFT_CAPTURE_ADDITIONAL_RECORDS").is_some() {
        std::fs::write(path, bytes).unwrap();
    } else {
        assert_eq!(bytes, std::fs::read(path).unwrap(), "{name}");
    }
}

#[test]
fn metadata_report_and_query_original_writers() {
    let bytes = metadata::encode(&Default::default()).unwrap();
    golden("metadata-manifest.json", &bytes[0]);
    let record = DocumentRecord::empty("input.fasta");
    let report = record.report().unwrap();
    let folder = tempfile::tempdir().unwrap();
    report.export(folder.path()).unwrap();
    for (file, name) in [
        ("record.json", "document-record.json"),
        ("report.json", "document-report.json"),
    ] {
        golden(name, &std::fs::read(folder.path().join(file)).unwrap());
    }
    let plan = blast::prepare(
        &[blast::Query {
            name: "sample".into(),
            sequence: "ACGT".into(),
            source: serde_json::Value::Null,
        }],
        &json!({"alphabet":"nucleotide","database":"nt"}),
    )
    .unwrap();
    golden(
        "query-preparation.json",
        &serde_json::to_vec(&plan).unwrap(),
    );
}

#[test]
fn provenance_lookup_returns_existing_bundle_then_nearest_project_then_fallback() {
    let root = tempfile::tempdir().unwrap();
    let result = root.path().join("result.fa");
    std::fs::write(&result, ">sample\nACGT\n").unwrap();
    let standalone = basecraft_ops::provenance::companion(&result);
    assert_eq!(
        basecraft_ops::provenance::bundle_for_result(&result),
        standalone
    );
    std::fs::create_dir_all(root.path().join(".basecraft")).unwrap();
    std::fs::write(root.path().join(".basecraft/project.toml"), "").unwrap();
    let expected = root
        .path()
        .join(".basecraft/provenance")
        .join(basecraft_io::hash_bytes(b"result.fa").trim_start_matches("b3:"));
    assert_eq!(
        basecraft_ops::provenance::bundle_for_result(&result),
        expected
    );
    std::fs::create_dir(&standalone).unwrap();
    std::fs::write(standalone.join("manifest.json"), "{}").unwrap();
    assert_eq!(
        basecraft_ops::provenance::bundle_for_result(&result),
        standalone
    );
}

#[test]
fn journal_writer_original_bytes() {
    let fixture: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../testdata/fixtures/records-v1/recovery.json"
    ))
    .unwrap();
    let entry: basecraft_protocol::LogEntry =
        serde_json::from_value(fixture["baseline"].clone()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut log = basecraft_ops::log::LogWriter::new(root.path(), "fixture");
    log.append(&entry).unwrap();
    drop(log);
    let path = std::fs::read_dir(basecraft_ops::log::log_dir(root.path()).join("fixture"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = std::fs::read(path).unwrap();
    golden("journal-entry.jsonl", &bytes);
    let record: basecraft_protocol::records::JournalEntry = serde_json::from_slice(&bytes).unwrap();
    let mut roundtrip = serde_json::to_vec(&serde_json::to_value(record).unwrap()).unwrap();
    roundtrip.push(b'\n');
    assert_eq!(roundtrip, bytes);
}

#[test]
fn sanger_invocation_original_records() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.ab1");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &source,
    )
    .unwrap();
    let run = basecraft_ops::sanger_review::SangerRun::prepare(
        &root.path().join("review.sanger-review.json"),
        "fixture",
        std::slice::from_ref(&source),
    )
    .unwrap();
    for (file, name) in [
        ("request.json", "sanger-request.json"),
        ("execution.json", "sanger-execution.json"),
    ] {
        let bytes = std::fs::read(run.directory().join(file)).unwrap();
        if std::env::var_os("BASECRAFT_CAPTURE_ADDITIONAL_RECORDS").is_some() {
            golden(name, &bytes);
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["operation"], "fixture");
    }
    run.failed("fixture failure").unwrap();
    let mut review = basecraft_ops::sanger_review::instrument_review(&source).unwrap();
    review.start_recovery().unwrap();
    let path = review
        .recovery_path()
        .unwrap()
        .parent()
        .unwrap()
        .join("manifest.json");
    if std::env::var_os("BASECRAFT_CAPTURE_ADDITIONAL_RECORDS").is_some() {
        golden("sanger-recovery.json", &std::fs::read(path).unwrap());
    }
}

#[test]
fn embedded_activity_original_records() {
    use basecraft_ops::Session;
    use basecraft_protocol::Op;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("support.nwk");
    let target = root.path().join("target.nwk");
    std::fs::write(&source, "((A:1,B:1)75:1,(C:1,D:1)60:1);").unwrap();
    std::fs::write(&target, "((A:1,B:1)90:1,(C:1,D:1)80:1);").unwrap();
    let mut session = Session::new(root.path());
    let doc = session.open(&target).unwrap();
    session.import_support(doc, &source, "Bootstrap").unwrap();
    for event in &session.document_record(doc).unwrap().events {
        if let Op::ToolRun { params, .. } = &event.op {
            for (key, name) in [
                ("input_run", "activity-input-run.json"),
                ("contributions", "activity-contributions.json"),
            ] {
                assert_eq!(params[key]["schema_version"], 1);
                if std::env::var_os("BASECRAFT_CAPTURE_ADDITIONAL_RECORDS").is_some() {
                    golden(name, &serde_json::to_vec(&params[key]).unwrap());
                }
            }
        }
    }
    let doc = session.create_sequence("sample", "ACGTACGT").unwrap();
    let extracted = root.path().join("extracted.fa");
    session
        .extract(doc, (1, 5), &[], false, &extracted)
        .unwrap();
    let bundle = basecraft_ops::provenance::bundle_for_result(&extracted);
    let input = basecraft_ops::input_evidence::read_input(&bundle).unwrap();
    assert_eq!(input.extra["extraction"]["schema_version"], 1);
    if std::env::var_os("BASECRAFT_CAPTURE_ADDITIONAL_RECORDS").is_some() {
        golden(
            "extraction.json",
            &serde_json::to_vec(&input.extra["extraction"]).unwrap(),
        );
    }
}
