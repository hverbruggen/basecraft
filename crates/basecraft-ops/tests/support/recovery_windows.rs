//! Strict native Windows recovery fixtures. Explicit ignored runs must execute
//! a real isolated child, not a default child invocation that skips the test.
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub fn enter(name: &str) -> Option<PathBuf> {
    if std::env::var("BASECRAFT_ISOLATED_CASE").as_deref() == Ok(name) {
        return Some(PathBuf::from(
            std::env::var_os("BASECRAFT_ISOLATED_ROOT").unwrap(),
        ));
    }
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--ignored", "--nocapture"])
        .env("BASECRAFT_ISOLATED_CASE", name)
        .env("BASECRAFT_ISOLATED_ROOT", root.path())
        .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
        .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
        .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
        .env("TMPDIR", root.path())
        .env("TMP", root.path())
        .env("TEMP", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "strict Windows fixture {name}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    None
}

pub fn assert_reparse(path: &Path) {
    use std::os::windows::fs::MetadataExt;
    // FILE_ATTRIBUTE_REPARSE_POINT, without adding a test-only FFI dependency.
    assert_ne!(
        std::fs::symlink_metadata(path).unwrap().file_attributes() & 0x400,
        0
    );
}

pub fn competitor(destination: &Path, external: &Path, junction: bool) {
    if junction {
        let output = Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(destination)
            .arg(external)
            .output()
            .expect("native Windows cmd.exe required for junction fixture");
        assert!(
            output.status.success(),
            "junction fixture required: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        std::os::windows::fs::symlink_file(external.join("missing"), destination)
            .expect("strict symlink fixture requires privilege or Developer Mode");
    }
    assert_reparse(destination);
}
