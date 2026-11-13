//! Select a fresh profile before any Session/AppPaths discovery. No unsafe
//! process-global environment mutation; sibling tests may run in parallel.
use std::{path::PathBuf, process::Command};

pub fn enter(name: &str) -> Option<PathBuf> {
    if std::env::var("BASECRAFT_ISOLATED_CASE").as_deref() == Ok(name) {
        return Some(PathBuf::from(
            std::env::var_os("BASECRAFT_ISOLATED_ROOT").unwrap(),
        ));
    }
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
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
        "{name}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if std::env::var_os("BASECRAFT_TEST_REPORT").is_some() {
        eprint!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    None
}

#[allow(unused_macros)]
macro_rules! isolate {
    () => {
        if $crate::isolated_profile::enter(std::thread::current().name().expect("test thread"))
            .is_none()
        {
            return;
        }
    };
}
#[allow(unused_imports)]
pub(crate) use isolate;
