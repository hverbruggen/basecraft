#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::records::ReportExt;
use basecraft_ops::{blast, metadata, records::DocumentRecord};
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
    crate::isolated_profile::isolate!();
    let bytes =
        metadata::encode(&basecraft_protocol::metadata::ProjectMetadata::default()).unwrap();
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
fn provenance_lookup_uses_the_chosen_project_only() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let result = root.path().join("result.fa");
    std::fs::write(&result, ">sample\nACGT\n").unwrap();
    let standalone = basecraft_ops::records::companion(&result);
    assert_eq!(
        basecraft_ops::records::bundle_for_result(&result, None),
        standalone
    );
    // A .basecraft folder makes nothing a project; only the user's choice does.
    std::fs::create_dir_all(root.path().join(".basecraft")).unwrap();
    assert_eq!(
        basecraft_ops::records::bundle_for_result(&result, None),
        standalone
    );
    let project = basecraft_ops::project::Project::new(root.path()).unwrap();
    let expected = project
        .root()
        .join(".basecraft/provenance")
        .join(basecraft_io::hash_bytes(b"result.fa").trim_start_matches("b3:"));
    // The project's bundle counts once it holds history.
    assert_eq!(
        basecraft_ops::records::bundle_for_result(&result, Some(&project)),
        standalone
    );
    std::fs::create_dir_all(&expected).unwrap();
    std::fs::write(expected.join("manifest.json"), "{}").unwrap();
    assert_eq!(
        basecraft_ops::records::bundle_for_result(&result, Some(&project)),
        expected
    );
    std::fs::create_dir(&standalone).unwrap();
    std::fs::write(standalone.join("manifest.json"), "{}").unwrap();
    assert_eq!(
        basecraft_ops::records::bundle_for_result(&result, Some(&project)),
        standalone
    );
}

#[test]
fn journal_writer_original_bytes() {
    crate::isolated_profile::isolate!();
    // The writer adds the checksum and sequence; the entry is the rest.
    let mut fixture: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../testdata/fixtures/records-v1/journal-entry.jsonl"
    ))
    .unwrap();
    let fields = fixture.as_object_mut().unwrap();
    fields.remove("_checksum");
    fields.remove("_sequence");
    let entry: basecraft_protocol::LogEntry = serde_json::from_value(fixture).unwrap();
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
    crate::isolated_profile::isolate!();
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
        None,
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
        let operation = if file == "execution.json" {
            "basecraft.sanger-review/fixture"
        } else {
            "fixture"
        };
        assert_eq!(value["operation"], operation);
    }
    run.failed("fixture failure").unwrap();
}

#[test]
fn embedded_activity_original_records() {
    use basecraft_ops::Session;
    use basecraft_protocol::Op;
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("support.nwk");
    let target = root.path().join("target.nwk");
    std::fs::write(&source, "((A:1,B:1)75:1,(C:1,D:1)60:1);").unwrap();
    std::fs::write(&target, "((A:1,B:1)90:1,(C:1,D:1)80:1);").unwrap();
    let mut session = Session::new(root.path());
    let doc = session.open(&target).unwrap();
    session.import_support(doc, &source, "Bootstrap").unwrap();
    for event in &session.document_record(doc).unwrap().events {
        if let Op::ToolRun {
            input_run,
            contributions,
            ..
        } = &event.op
        {
            for (value, name) in [
                (
                    serde_json::to_value(input_run).unwrap(),
                    "activity-input-run.json",
                ),
                (
                    serde_json::to_value(contributions).unwrap(),
                    "activity-contributions.json",
                ),
            ] {
                assert_eq!(value["schema_version"], 1);
                if std::env::var_os("BASECRAFT_CAPTURE_ADDITIONAL_RECORDS").is_some() {
                    golden(name, &serde_json::to_vec(&value).unwrap());
                }
            }
        }
    }
    let doc = session.create_sequence("sample", "ACGTACGT").unwrap();
    let extracted = root.path().join("extracted.fa");
    session
        .extract(doc, (1, 5), &[], false, &extracted, None)
        .unwrap();
    let bundle = basecraft_ops::records::bundle_for_result(&extracted, None);
    let input = basecraft_ops::records::read_input(&bundle).unwrap();
    assert_eq!(input.extra["extraction"]["schema_version"], 1);
    if std::env::var_os("BASECRAFT_CAPTURE_ADDITIONAL_RECORDS").is_some() {
        golden(
            "extraction.json",
            &serde_json::to_vec(&input.extra["extraction"]).unwrap(),
        );
    }
}
