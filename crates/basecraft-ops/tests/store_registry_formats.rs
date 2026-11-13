//! Byte oracles run against original writers before their typed substitution.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{records, records::ArtifactRun};
use basecraft_platform::paths::AppPaths;
use serde_json::json;
use std::{path::Path, process::Command};

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

    // The index beside an analysed file names the store, relative to its folder.
    let data = root.join("data");
    std::fs::create_dir(&data).unwrap();
    let source = data.join("reads.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    std::fs::create_dir(root.join("results")).unwrap();
    records::reference_input_with_dependencies(
        &root.join("results/tree.nwk"),
        &source,
        &basecraft_io::hash_file(&source).unwrap(),
        &[],
        "index format",
        None,
    )
    .unwrap();
    let index = data.join(".basecraft/analysis-index.json");
    let expected = r#"{
  "schema_version": 1,
  "files": {
    "reads.fasta": [
      "../results/.basecraft/analyses-v1"
    ]
  }
}
"#;
    assert_eq!(std::fs::read_to_string(&index).unwrap(), expected);
    // Nothing about the analysis is kept in basecraft's own folders.
    let state = AppPaths::discover().unwrap().state;
    for entry in walk(&state) {
        assert!(
            entry.parent().is_some_and(|p| p.ends_with("locks")),
            "{entry:?}"
        );
    }
    // A damaged index stops new analyses of files beside it and is left as is.
    std::fs::write(&index, b"{broken").unwrap();
    assert!(
        records::reference_input_with_dependencies(
            &root.join("results/tree.nwk"),
            &source,
            &basecraft_io::hash_file(&source).unwrap(),
            &[],
            "index format",
            None,
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&index).unwrap(), b"{broken");
    assert!(!records::check(&source).unwrap().is_complete());
}

fn walk(folder: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        if entry.file_type().unwrap().is_dir() {
            files.extend(walk(&entry.path()));
        } else {
            files.push(entry.path());
        }
    }
    files
}

#[test]
fn committed_store_examples_round_trip() {
    use basecraft_protocol::records::StoreMarker;
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
}
