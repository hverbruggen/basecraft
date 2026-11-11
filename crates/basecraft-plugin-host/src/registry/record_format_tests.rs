//! Original pre-substitution LC01 bytes/readers, and serializer-only LC02 evidence.
//! Synthetic inputs never provision, discover, install, verify or execute a runtime.
use super::*;
use serde_json::json;

// Frozen old declaration-order serializers. Never derive these from the production
// types or emitted JSON: field order is part of the byte/compact-identity oracle.
#[derive(Serialize)]
struct OldRuntime {
    schema: u32,
    executables: Vec<String>,
    root: PathBuf,
    interpreter: PathBuf,
    lock_digest: String,
    manifest_digest: String,
    package_digest: String,
    reported_versions: BTreeMap<String, String>,
    executable_digests: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct OldIndex {
    schema: u32,
    active: BTreeMap<String, String>,
    runtimes: BTreeMap<String, OldRuntime>,
    disabled: BTreeSet<String>,
    editable: BTreeSet<PathBuf>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

fn old_empty(schema: u32) -> OldIndex {
    OldIndex {
        schema,
        active: BTreeMap::new(),
        runtimes: BTreeMap::new(),
        disabled: BTreeSet::new(),
        editable: BTreeSet::new(),
        unknown: BTreeMap::new(),
    }
}

// All path spellings are predeclared nonpersonal inputs, never normalized or
// redacted from a real capture. Windows-looking strings on Unix prove escaping only.
fn original_runtime(populated: bool) -> crate::runtime::Runtime {
    crate::runtime::Runtime(basecraft_protocol::installation_records::Runtime {
        schema: 1,
        executables: vec!["z-tool".into(), "a-tool".into(), "z-tool".into()],
        root: "portable/环境 \"root\"".into(),
        interpreter: r"C:\portable\bin\python.exe".into(),
        lock_digest: "sha256:lock".into(),
        manifest_digest: "sha256:manifest".into(),
        package_digest: "sha256:package".into(),
        reported_versions: if populated {
            BTreeMap::from([
                ("z-tool".into(), "v2\n\t\"é".into()),
                ("a-tool".into(), "1".into()),
            ])
        } else {
            BTreeMap::new()
        },
        executable_digests: if populated {
            BTreeMap::from([
                ("z-tool".into(), "sha256:z".into()),
                ("a-tool".into(), "sha256:a".into()),
            ])
        } else {
            BTreeMap::new()
        },
    })
}

fn old_runtime(populated: bool) -> OldRuntime {
    OldRuntime {
        schema: 1,
        executables: vec!["z-tool".into(), "a-tool".into(), "z-tool".into()],
        root: PathBuf::from("portable/环境 \"root\""),
        interpreter: PathBuf::from(r"C:\portable\bin\python.exe"),
        lock_digest: String::from("sha256:lock"),
        manifest_digest: String::from("sha256:manifest"),
        package_digest: String::from("sha256:package"),
        reported_versions: if populated {
            [
                (String::from("a-tool"), String::from("1")),
                (String::from("z-tool"), String::from("v2\n\t\"é")),
            ]
            .into()
        } else {
            BTreeMap::new()
        },
        executable_digests: if populated {
            [
                (String::from("a-tool"), String::from("sha256:a")),
                (String::from("z-tool"), String::from("sha256:z")),
            ]
            .into()
        } else {
            BTreeMap::new()
        },
    }
}

fn populated() -> InstallationIndex {
    InstallationIndex {
        schema: 1,
        active: [
            ("z.plugin".into(), "z-package".into()),
            ("a.plugin".into(), "a-package".into()),
        ]
        .into(),
        runtimes: [
            ("z-package".into(), original_runtime(false)),
            ("a-package".into(), original_runtime(true)),
        ]
        .into(),
        disabled: ["z.plugin".into(), "a.plugin".into()].into(),
        editable: [PathBuf::from("z/é"), PathBuf::from("a/\"quoted\"\\path")].into(),
        unknown: [
            ("z-extra".into(), json!([null, true, 1.25])),
            ("a-extra".into(), json!({"z":2,"a":["é",false]})),
            ("middle-extra".into(), json!("\n\t\\\"")),
        ]
        .into(),
    }
}

fn old_populated() -> OldIndex {
    OldIndex {
        schema: 1,
        active: [
            ("a.plugin".into(), "a-package".into()),
            ("z.plugin".into(), "z-package".into()),
        ]
        .into(),
        runtimes: [
            ("a-package".into(), old_runtime(true)),
            ("z-package".into(), old_runtime(false)),
        ]
        .into(),
        disabled: [String::from("a.plugin"), String::from("z.plugin")].into(),
        editable: [PathBuf::from("a/\"quoted\"\\path"), PathBuf::from("z/é")].into(),
        unknown: [
            ("a-extra".into(), json!({"a":["é",false],"z":2})),
            ("middle-extra".into(), json!("\n\t\\\"")),
            ("z-extra".into(), json!([null, true, 1.25])),
        ]
        .into(),
    }
}

fn config(root: &Path) -> RegistryConfig {
    RegistryConfig {
        bundled: root.join("unused-bundled"),
        packages: root.join("unused-packages"),
        index: root.join("config/plugins-v1.json"),
    }
}

fn fixture(name: &str, actual: &[u8]) {
    assert_ne!(actual.last(), Some(&b'\n'), "no appended LF: {name}");
    if let Some(root) = std::env::var_os("BASECRAFT_INSTALLATION_RECORD_CAPTURE") {
        // Explicit capture mode writes only to the allocated external evidence root.
        let root = PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(format!("{name}.json")))
            .unwrap();
        std::io::Write::write_all(&mut file, actual).unwrap();
    } else {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/records-v1")
            .join(format!("{name}.json"));
        assert_eq!(
            actual,
            std::fs::read(path).unwrap(),
            "frozen capture: {name}"
        );
    }
}

fn index_bytes(config: &RegistryConfig, name: &str, old: &OldIndex) {
    let actual = std::fs::read(&config.index).unwrap();
    assert_eq!(
        actual,
        serde_json::to_vec_pretty(old).unwrap(),
        "independent ordered oracle: {name}"
    );
    fixture(name, &actual);
}

#[test]
fn original_index_writer_bytes_and_synthetic_mutations() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    assert_eq!(load_index(&config).unwrap().schema, 1);
    assert!(
        !config.index.parent().unwrap().exists(),
        "passive missing-file read must not create directories"
    );
    write_index(&config, |_| Ok(())).unwrap();
    index_bytes(&config, "installation-index-empty", &old_empty(1));
    fixture(
        "installation-index-rust-default",
        &serde_json::to_vec_pretty(&InstallationIndex::default()).unwrap(),
    );
    assert_eq!(
        serde_json::to_vec_pretty(&InstallationIndex::default()).unwrap(),
        serde_json::to_vec_pretty(&old_empty(0)).unwrap()
    );
    write_index(&config, |index| {
        *index = populated();
        Ok(())
    })
    .unwrap();
    let mut old = old_populated();
    index_bytes(&config, "installation-index-populated", &old);

    // These closures simulate state changes through the real writer. They are
    // not calls to install/remove/set_enabled, and prove no installation event.
    write_index(&config, |index| {
        index.disabled.remove("a.plugin");
        Ok(())
    })
    .unwrap();
    old.disabled.remove("a.plugin");
    index_bytes(&config, "installation-index-mutations-enable", &old);
    write_index(&config, |index| {
        index.disabled.insert("a.plugin".into());
        index.active.remove("a.plugin");
        Ok(())
    })
    .unwrap();
    old.disabled.insert("a.plugin".into());
    old.active.remove("a.plugin");
    index_bytes(&config, "installation-index-mutations-remove", &old);
    assert_eq!(load_index(&config).unwrap().runtimes.len(), 2);
    write_index(&config, |index| {
        index.active.insert("a.plugin".into(), "new-package".into());
        index
            .runtimes
            .insert("new-package".into(), original_runtime(false));
        index.editable.remove(Path::new("z/é"));
        Ok(())
    })
    .unwrap();
    old.active.insert("a.plugin".into(), "new-package".into());
    old.runtimes
        .insert("new-package".into(), old_runtime(false));
    old.editable.remove(Path::new("z/é"));
    index_bytes(&config, "installation-index-mutations-reselect", &old);

    // Seed deliberately omits schema/index defaults and trailing runtime maps.
    std::fs::write(&config.index, r#"{"runtimes":{"defaulted":{"schema":1,"executables":[],"root":"","interpreter":"","lock_digest":"","manifest_digest":"","package_digest":""}},"extra":{"z":1,"a":2}}"#).unwrap();
    write_index(&config, |_| Ok(())).unwrap();
    let mut expected = old_empty(1);
    expected.runtimes.insert(
        "defaulted".into(),
        OldRuntime {
            schema: 1,
            executables: vec![],
            root: PathBuf::new(),
            interpreter: PathBuf::new(),
            lock_digest: String::new(),
            manifest_digest: String::new(),
            package_digest: String::new(),
            reported_versions: BTreeMap::new(),
            executable_digests: BTreeMap::new(),
        },
    );
    expected
        .unknown
        .insert("extra".into(), json!({"a":2,"z":1}));
    index_bytes(&config, "installation-index-defaulted", &expected);
}

#[test]
fn original_runtime_serializer_only_and_compact_identity() {
    for populated in [false, true] {
        let mut actual = Vec::new();
        // Same call shape as provision's runtime.json writer; no provisioning.
        serde_json::to_writer_pretty(&mut actual, &original_runtime(populated)).unwrap();
        assert_eq!(
            actual,
            serde_json::to_vec_pretty(&old_runtime(populated)).unwrap()
        );
        fixture(
            if populated {
                "managed-plugin-runtime-serializer-populated"
            } else {
                "managed-plugin-runtime-serializer-empty-maps"
            },
            &actual,
        );
    }
    #[derive(Serialize)]
    struct OldDiagnostic {
        path: PathBuf,
        message: String,
    }
    // Synthetic tuples in the exact registry generation call shape, including
    // managed_runtime. No Registry::load, manifest discovery or semantic verify.
    let runtime = original_runtime(true);
    let identity = vec![
        (
            "a.plugin",
            "sha256:a",
            false,
            Some(PathBuf::from("bin/python")),
            Some(&runtime),
        ),
        ("z.plugin", "sha256:z", true, None, None),
    ];
    let diagnostics = vec![Diagnostic {
        path: "a/package".into(),
        message: "synthetic diagnostic".into(),
    }];
    let actual = serde_json::to_vec(&(&identity, &diagnostics)).unwrap();
    let old_runtime = old_runtime(true);
    let old_identity = vec![
        (
            "a.plugin",
            "sha256:a",
            false,
            Some(PathBuf::from("bin/python")),
            Some(&old_runtime),
        ),
        ("z.plugin", "sha256:z", true, None, None),
    ];
    let old_diagnostics = vec![OldDiagnostic {
        path: "a/package".into(),
        message: "synthetic diagnostic".into(),
    }];
    let expected = serde_json::to_vec(&(&old_identity, &old_diagnostics)).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        basecraft_io::hash_bytes(&actual),
        basecraft_io::hash_bytes(&expected)
    );
    fixture("installation-index-identity-compact", &actual);
}

fn outcome(result: Result<InstallationIndex>) -> Value {
    match result {
        Ok(index) => json!({"kind":"Ok", "value":serde_json::to_value(index).unwrap()}),
        Err(error) => error_outcome(error),
    }
}

fn error_outcome(error: Error) -> Value {
    match &error {
        Error::Json(e) => {
            json!({"kind":"Json", "display":error.to_string(), "line":e.line(), "column":e.column(), "category":format!("{:?}",e.classify())})
        }
        Error::Protocol(_) => json!({"kind":"Protocol", "display":error.to_string()}),
        other => panic!("unexpected original capture error: {other:?}"),
    }
}

#[test]
fn committed_index_reader_cases() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    std::fs::create_dir_all(config.index.parent().unwrap()).unwrap();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../testdata/fixtures/records-v1/installation-index-reader-observations.json"
    ))
    .unwrap();
    for case in cases {
        match case["case"].as_str().unwrap() {
            "missing-file" => {}
            "input-exact-limit" | "input-over-limit-before-parse" => {
                let mut bytes = b"{}".to_vec();
                bytes.resize(MAX_DESCRIPTOR_BYTES, b' ');
                if case["case"] == "input-over-limit-before-parse" {
                    bytes.push(b'!');
                }
                std::fs::write(&config.index, bytes).unwrap();
            }
            _ => {
                std::fs::write(&config.index, case["input"].as_str().unwrap()).unwrap();
            }
        }
        assert_eq!(
            outcome(load_index(&config)),
            case["outcome"],
            "{}",
            case["case"]
        );
    }
}

#[cfg(unix)]
#[test]
fn original_writer_limits_and_reached_failure_stages() {
    assert!(
        cfg!(unix),
        "Unix native-path evidence requires Unix; qualify other targets separately"
    );
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    write_index(&config, |_| Ok(())).unwrap();
    let original = std::fs::read(&config.index).unwrap();
    let mut observations = Vec::new();
    let mut expected = old_empty(1);
    expected.unknown.insert("padding".into(), json!(""));
    let overhead = serde_json::to_vec_pretty(&expected).unwrap().len();
    let padding = "x".repeat(MAX_DESCRIPTOR_BYTES - overhead);
    expected.unknown.insert("padding".into(), json!(padding));
    write_index(&config, |index| {
        index.unknown.insert("padding".into(), json!(padding));
        Ok(())
    })
    .unwrap();
    let exact = std::fs::read(&config.index).unwrap();
    assert_eq!(exact.len(), MAX_DESCRIPTOR_BYTES);
    assert_eq!(exact, serde_json::to_vec_pretty(&expected).unwrap());
    observations.push(json!({"stage":"exact-output-limit", "bytes":exact.len(), "kind":"Ok"}));
    let error = write_index(&config, |index| {
        index
            .unknown
            .insert("padding".into(), json!(format!("{padding}x")));
        Ok(())
    })
    .unwrap_err();
    observations.push(json!({"stage":"serialization-size-check-before-atomic-write", "outcome":error_outcome(error)}));
    assert_eq!(std::fs::read(&config.index).unwrap(), exact);
    std::fs::write(&config.index, &original).unwrap();
    let error = write_index(&config, |_| {
        Err(Error::Protocol("synthetic closure refusal".into()))
    })
    .unwrap_err();
    observations.push(
        json!({"stage":"change-closure-before-serialization", "outcome":error_outcome(error)}),
    );
    assert_eq!(std::fs::read(&config.index).unwrap(), original);
    std::fs::write(&config.index, b"null").unwrap();
    let reached = std::cell::Cell::new(false);
    let error = write_index(&config, |_| {
        reached.set(true);
        Ok(())
    })
    .unwrap_err();
    assert!(!reached.get());
    assert_eq!(std::fs::read(&config.index).unwrap(), b"null");
    observations
        .push(json!({"stage":"load-before-change-closure", "outcome":error_outcome(error)}));
    assert!(config.index.with_extension("lock").exists());
    std::fs::write(&config.index, &original).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let invalid = PathBuf::from(std::ffi::OsString::from_vec(vec![b'x', 0xff]));
        let error = write_index(&config, |index| {
            index.editable.insert(invalid);
            Ok(())
        })
        .unwrap_err();
        observations.push(json!({"stage":"native-path-serialization-before-atomic-write", "outcome":error_outcome(error)}));
        assert_eq!(std::fs::read(&config.index).unwrap(), original);
    }
    fixture(
        "installation-index-writer-observations",
        &serde_json::to_vec_pretty(&observations).unwrap(),
    );
}

#[test]
fn substituted_host_alias_and_transparent_receipts_round_trip() {
    // Runtime deliberately implements neither Default nor JsonSchema.
    assert_eq!(InstallationIndex::default().schema, 0);
    for bytes in [
        include_bytes!("../../../../testdata/fixtures/records-v1/installation-index-empty.json")
            .as_slice(),
        include_bytes!(
            "../../../../testdata/fixtures/records-v1/installation-index-populated.json"
        )
        .as_slice(),
        include_bytes!(
            "../../../../testdata/fixtures/records-v1/installation-index-defaulted.json"
        )
        .as_slice(),
        include_bytes!(
            "../../../../testdata/fixtures/records-v1/installation-index-mutations-enable.json"
        )
        .as_slice(),
        include_bytes!(
            "../../../../testdata/fixtures/records-v1/installation-index-mutations-remove.json"
        )
        .as_slice(),
        include_bytes!(
            "../../../../testdata/fixtures/records-v1/installation-index-mutations-reselect.json"
        )
        .as_slice(),
        include_bytes!(
            "../../../../testdata/fixtures/records-v1/installation-index-rust-default.json"
        )
        .as_slice(),
    ] {
        let record: InstallationIndex = serde_json::from_slice(bytes).unwrap();
        assert_eq!(serde_json::to_vec_pretty(&record).unwrap(), bytes);
    }
    for bytes in [
        include_bytes!("../../../../testdata/fixtures/records-v1/managed-plugin-runtime-serializer-empty-maps.json").as_slice(),
        include_bytes!("../../../../testdata/fixtures/records-v1/managed-plugin-runtime-serializer-populated.json").as_slice(),
    ] {
        let record: crate::runtime::Runtime = serde_json::from_slice(bytes).unwrap();
        assert_eq!(serde_json::to_vec_pretty(&record).unwrap(), bytes);
    }
}
