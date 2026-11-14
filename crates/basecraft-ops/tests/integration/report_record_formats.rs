//! Original writer oracles intentionally use literal JSON, independently of protocol types.
use basecraft_ops::records::{DocumentRecord, MethodsParagraph, Report, ReportExt};
use serde_json::{Value, json};
use std::path::Path;

fn pretty(value: &Value, lf: bool) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    if lf {
        bytes.push(b'\n');
    }
    bytes
}
fn map(report: &Report) -> Value {
    Value::Array(
        report
            .paragraphs
            .iter()
            .enumerate()
            .map(|(i, p)| json!({"paragraph":i+1,"activities":p.activity_ids}))
            .collect(),
    )
}
fn export_manifest(report: &Report) -> Value {
    json!({"schema_version":1,"revision":report.revision,"document":report.document,"unsaved":report.unsaved,"template_version":report.template_version,"coordinates":"raw zero-based half-open; summaries one-based"})
}
fn check_file(directory: &Path, name: &str, expected: &[u8], capture: Option<&str>) {
    let bytes = std::fs::read(directory.join(name)).unwrap();
    assert_eq!(bytes, expected, "{name}");
    if let (Some(root), Some(prefix)) = (std::env::var_os("BASECRAFT_REPORT_CAPTURE"), capture) {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(format!("{prefix}-{name}")), bytes).unwrap();
    }
}
fn check_export(report: &Report, directory: &Path, capture: Option<&str>) {
    report.export(directory).unwrap();
    check_file(
        directory,
        "manifest.json",
        &basecraft_protocol::records::encode(&export_manifest(report)).unwrap(),
        capture,
    );
    check_file(
        directory,
        "source-map.json",
        &pretty(&map(report), false),
        capture,
    );
    check_file(
        directory,
        "citations.json",
        &pretty(&json!(report.citations), false),
        capture,
    );
    assert_eq!(
        std::fs::read(directory.join("record.json")).unwrap(),
        report.raw.as_bytes()
    );
    assert_eq!(
        std::fs::read(directory.join("report.json")).unwrap(),
        basecraft_protocol::records::encode(report).unwrap()
    );
}
fn custom_report() -> Report {
    let mut report = DocumentRecord::empty("C:\\fixture\\λ\"\n.fasta")
        .report()
        .unwrap();
    report.schema_version = 99;
    report.template_version = 73;
    report.revision = "revision\\\"\tλ".into();
    report.unsaved = true;
    report.raw = "opaque raw text, intentionally not JSON\n".into();
    report.paragraphs = vec![
        MethodsParagraph {
            activity_ids: vec!["z\\\"\n".into(), "a".into(), "z\\\"\n".into()],
            text: "custom".into(),
            parameters: "seed=42\tλ".into(),
        },
        MethodsParagraph {
            activity_ids: vec![],
            text: "empty".into(),
            parameters: String::new(),
        },
    ];
    report.citations = vec!["Z λ\"\n\\".into(), "A".into(), "Z λ\"\n\\".into()];
    report
}
#[test]
fn export_selected_ownership_acceptance_and_refusal() {
    crate::isolated_profile::isolate!();
    let report = custom_report();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for marker in [
        json!({"schema_version":1,"template_version":null,"coordinates":null}),
        json!({"schema_version":1,"template_version":[],"coordinates":42,"document":"another","revision":false,"future":{"x":1}}),
    ] {
        let folder = tempfile::tempdir_in(root).unwrap();
        std::fs::write(folder.path().join("history.md"), b"foreign history").unwrap();
        std::fs::write(
            folder.path().join("manifest.json"),
            basecraft_protocol::records::encode(&marker).unwrap(),
        )
        .unwrap();
        std::fs::write(folder.path().join("methods.md"), b"authored\r\n").unwrap();
        std::fs::write(folder.path().join("unrelated"), b"keep").unwrap();
        check_export(&report, folder.path(), None);
        check_export(&report, folder.path(), None);
        assert_eq!(
            std::fs::read(folder.path().join("methods.md")).unwrap(),
            b"authored\r\n"
        );
        assert_eq!(
            std::fs::read(folder.path().join("unrelated")).unwrap(),
            b"keep"
        );
    }
    let folder = root.join("authored-only");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("methods.md"), b"authored").unwrap();
    check_export(&report, &folder, None);
    assert_eq!(
        std::fs::read(folder.join("methods.md")).unwrap(),
        b"authored"
    );
    let reserved = [
        "methods.generated.md",
        "history.md",
        "record.json",
        "parameters.tsv",
        "source-map.json",
        "citations.json",
        "manifest.json",
        "report.json",
    ];
    let bad = [
        None,
        Some(b"invalid".to_vec()),
        Some(pretty(
            &json!({"schema_version":2,"template_version":2,"coordinates":"x"}),
            false,
        )),
        Some(pretty(
            &json!({"schema_version":1,"coordinates":"x"}),
            false,
        )),
        Some(pretty(
            &json!({"schema_version":1,"template_version":2}),
            false,
        )),
    ];
    for name in reserved {
        for marker in &bad {
            let folder = tempfile::tempdir_in(root).unwrap();
            std::fs::write(folder.path().join(name), b"reserved original").unwrap();
            if let Some(bytes) = marker {
                std::fs::write(folder.path().join("manifest.json"), bytes).unwrap();
            }
            let before = snapshot(folder.path());
            let error = report.export(folder.path()).unwrap_err();
            assert!(matches!(error, basecraft_ops::Error::Invalid(_)));
            assert!(!error.to_string().contains("incomplete"));
            assert_eq!(snapshot(folder.path()), before);
        }
    }
}
fn snapshot(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            let bytes = if entry.file_type().unwrap().is_dir() {
                b"DIRECTORY".to_vec()
            } else {
                std::fs::read(entry.path()).unwrap()
            };
            (name, bytes)
        })
        .collect()
}
#[test]
fn export_partial_writes_keep_order_and_authored_text() {
    crate::isolated_profile::isolate!();
    let report = custom_report();
    let dir = tempfile::tempdir().unwrap();
    let generated = [
        "methods.generated.md",
        "history.md",
        "record.json",
        "parameters.tsv",
        "source-map.json",
        "citations.json",
        "manifest.json",
        "report.json",
    ];
    for (obstructed, stage) in [
        ("source-map.json", 4),
        ("citations.json", 5),
        ("report.json", 7),
    ] {
        let folder = tempfile::tempdir_in(dir.path()).unwrap();
        for name in generated {
            std::fs::write(folder.path().join(name), format!("old-{name}")).unwrap();
        }
        std::fs::write(
            folder.path().join("manifest.json"),
            basecraft_protocol::records::encode(
                &json!({"schema_version":1,"template_version":null,"coordinates":null}),
            )
            .unwrap(),
        )
        .unwrap();
        std::fs::write(folder.path().join("methods.md"), b"authored").unwrap();
        std::fs::remove_file(folder.path().join(obstructed)).unwrap();
        std::fs::create_dir(folder.path().join(obstructed)).unwrap();
        let before = snapshot(folder.path());
        let error = report.export(folder.path()).unwrap_err();
        assert!(matches!(error, basecraft_ops::Error::Damaged { .. }));
        assert!(error.to_string().contains("incomplete"));
        let after = snapshot(folder.path());
        let successful = tempfile::tempdir_in(dir.path()).unwrap();
        check_export(&report, successful.path(), None);
        for (i, name) in generated.iter().enumerate() {
            if i < stage {
                assert_eq!(
                    after[*name],
                    std::fs::read(successful.path().join(name)).unwrap(),
                    "{obstructed}: {name}"
                );
            } else {
                assert_eq!(after[*name], before[*name], "{obstructed}: {name}");
            }
        }
        assert_eq!(after["methods.md"], b"authored");
    }
}
#[test]
fn export_follows_existing_manifest_symlink() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker.json");
    let bytes = basecraft_protocol::records::encode(
        &json!({"schema_version":1,"template_version":null,"coordinates":null}),
    )
    .unwrap();
    std::fs::write(&marker, &bytes).unwrap();
    let folder = dir.path().join("export");
    std::fs::create_dir(&folder).unwrap();
    std::os::unix::fs::symlink(&marker, folder.join("manifest.json")).unwrap();
    check_export(&custom_report(), &folder, None);
    assert_eq!(std::fs::read(&marker).unwrap(), bytes);
    assert!(
        !std::fs::symlink_metadata(folder.join("manifest.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
