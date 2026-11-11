//! Byte oracles run against original writers before their typed substitution.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{analysis_store::ArtifactRun, input_evidence};
use basecraft_platform::paths::AppPaths;
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn pretty(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn capture(name: &str, bytes: &[u8]) {
    if let Some(directory) = std::env::var_os("BASECRAFT_STORE_CAPTURE") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(Path::new(&directory).join(name), bytes).unwrap();
    }
}

#[test]
fn store_writer_bytes_and_validation() {
    crate::isolated_profile::isolate!();
    if std::env::var_os("BASECRAFT_STORE_CHILD").is_none() {
        let root = tempfile::tempdir().unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store_writer_bytes_and_validation",
                "--nocapture",
            ])
            .env("BASECRAFT_STORE_CHILD", root.path())
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
            .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
            .env("BASECRAFT_SCRATCH_DIR", root.path())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("BASECRAFT_STORE_CHILD").unwrap());
    let run = ArtifactRun::create(&root.join("result.json"), "first").unwrap();
    let store = run.directory().parent().unwrap().parent().unwrap();
    let marker = store.join("store.json");
    let original = std::fs::read(&marker).unwrap();
    assert_eq!(
        original,
        b"{\"format\":\"basecraft.analysis-store\",\"schema_version\":1}\n"
    );
    capture("store.json", &original);
    ArtifactRun::validate_store(store).unwrap();
    for bad in [
        json!({"format":"foreign","schema_version":1}),
        json!({"format":"basecraft.analysis-store","schema_version":2}),
        json!({}),
        json!([]),
    ] {
        std::fs::write(&marker, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(
            ArtifactRun::validate_store(store)
                .unwrap_err()
                .to_string()
                .contains("foreign or unsupported analysis namespace; nothing was migrated")
        );
    }
    std::fs::write(&marker, vec![b' '; 4097]).unwrap();
    assert!(
        ArtifactRun::validate_store(store)
            .unwrap_err()
            .to_string()
            .contains("unsafe marker")
    );
    let extended_marker = b"{\"format\":\"basecraft.analysis-store\",\"future\":{\"enabled\":true},\"schema_version\":1}\n";
    std::fs::write(&marker, extended_marker).unwrap();
    ArtifactRun::validate_store(store).unwrap();
    capture("store-extended.json", extended_marker);

    let _lock = input_evidence::lock().unwrap();
    let registry = AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-stores.json");
    assert!(!registry.exists());
    input_evidence::register(&run).unwrap();
    let expected = json!({"schema_version":1,"stores":[store]});
    assert_eq!(std::fs::read(&registry).unwrap(), pretty(&expected));
    capture("registry-first.json", &std::fs::read(&registry).unwrap());
    // Missing registry defaults to the same empty shape, but never writes it.
    let empty = json!({"schema_version":1,"stores":[]});
    std::fs::write(&registry, pretty(&empty)).unwrap();
    capture("registry-empty.json", &std::fs::read(&registry).unwrap());
    input_evidence::register(&run).unwrap();
    assert_eq!(std::fs::read(&registry).unwrap(), pretty(&expected));
    let extended =
        json!({"schema_version":1,"stores":[],"z_extension":{"b":[null,{"extra":true}],"a":1}});
    std::fs::write(&registry, pretty(&extended)).unwrap();
    input_evidence::register(&run).unwrap();
    let mut expected = extended;
    expected["stores"] = json!([store]);
    let bytes = std::fs::read(&registry).unwrap();
    assert_eq!(bytes, pretty(&expected));
    capture("registry-extended.json", &bytes);
    input_evidence::register(&run).unwrap();
    assert_eq!(std::fs::read(&registry).unwrap(), bytes);
    let second_root = root.join("other");
    std::fs::create_dir(&second_root).unwrap();
    let second = ArtifactRun::create(&second_root.join("result.json"), "second").unwrap();
    let second_store = second.directory().parent().unwrap().parent().unwrap();
    input_evidence::register(&second).unwrap();
    expected["stores"] = json!([store, second_store]);
    assert_eq!(std::fs::read(&registry).unwrap(), pretty(&expected));
    capture("registry-update.json", &std::fs::read(&registry).unwrap());
    for bad in [
        json!({"schema_version":2,"stores":[]}),
        json!({"schema_version":1,"stores":{}}),
        json!([]),
    ] {
        std::fs::write(&registry, pretty(&bad)).unwrap();
        assert!(
            input_evidence::register(&run)
                .unwrap_err()
                .to_string()
                .contains("unsupported known-store registry")
        );
        assert_eq!(std::fs::read(&registry).unwrap(), pretty(&bad));
    }
    for paths in [json!(["relative/path"]), json!(vec![store; 4097])] {
        let bad = pretty(&json!({"schema_version":1,"stores":paths}));
        std::fs::write(&registry, &bad).unwrap();
        assert!(
            input_evidence::register(&run)
                .unwrap_err()
                .to_string()
                .contains("absolute and bounded")
        );
        assert_eq!(std::fs::read(&registry).unwrap(), bad);
    }
    std::fs::write(
        &registry,
        pretty(&json!({"schema_version":1,"stores":vec![second_store;4096]})),
    )
    .unwrap();
    assert!(
        input_evidence::register(&run)
            .unwrap_err()
            .to_string()
            .contains("registry limit reached")
    );
    std::fs::write(&registry, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(
        matches!(input_evidence::register(&run), Err(basecraft_ops::Error::Limit { limit, .. }) if limit == 1 << 20)
    );
    std::fs::write(&registry, b"{broken").unwrap();
    assert!(input_evidence::register(&run).is_err());
    std::fs::write(&registry, pretty(&json!({"schema_version":1,"stores":[7]}))).unwrap();
    assert!(input_evidence::register(&run).is_err());
}

#[test]
fn committed_store_examples_round_trip() {
    use basecraft_protocol::records::{StoreMarker, StoreRegistry};
    crate::isolated_profile::isolate!();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/records-v1");
    for name in ["store.json", "store-extended.json"] {
        let bytes = std::fs::read(fixtures.join(name)).unwrap();
        let marker: StoreMarker = serde_json::from_slice(&bytes).unwrap();
        let value = serde_json::to_value(marker).unwrap();
        let mut actual = serde_json::to_vec(&value).unwrap();
        actual.push(b'\n');
        assert_eq!(actual, bytes, "{name}");
    }
    for name in ["empty", "first", "extended", "update"] {
        let bytes = std::fs::read(fixtures.join(format!("store-registry-{name}.json"))).unwrap();
        let registry: StoreRegistry = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(registry.schema_version, 1);
        assert_eq!(
            pretty(&serde_json::to_value(registry).unwrap()),
            bytes,
            "{name}"
        );
    }
}
