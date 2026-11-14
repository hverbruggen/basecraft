//! Strict native Windows recovery fixtures. The tests enter their isolated
//! child through `isolated_profile::enter`, which also runs ignored tests.
use std::{path::Path, process::Command};

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
