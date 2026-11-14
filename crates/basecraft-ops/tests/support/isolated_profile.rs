//! Select a fresh profile before any Session/AppPaths discovery. No unsafe
//! process-global environment mutation; sibling tests may run in parallel.
use std::{
    path::PathBuf,
    process::{Command, Output},
};

/// The running test's full name, module path included, as `--exact` needs it.
/// Test binaries hold many modules, so a bare function name matches no test.
pub fn test_name() -> String {
    std::thread::current()
        .name()
        .expect("test thread")
        .to_owned()
}

/// Full name of another test in the running test's module.
#[allow(dead_code)]
pub fn sibling(test: &str) -> String {
    match test_name().rsplit_once("::") {
        Some((module, _)) => format!("{module}::{test}"),
        None => test.to_owned(),
    }
}

/// This test binary, set to run exactly `test`. The caller already chose the
/// test, so it runs even when it is ignored.
pub fn relaunch(test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", test, "--include-ignored", "--nocapture"]);
    command
}

/// Fails unless the relaunched child passed exactly one test. A name that
/// matches no test would otherwise pass having run nothing.
pub fn assert_one_passed(test: &str, output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "{test} ({}):\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Runs the current test again in a child process with its own profile and
/// returns `None` here; in that child it returns the child's root folder.
pub fn enter() -> Option<PathBuf> {
    let name = test_name();
    if std::env::var("BASECRAFT_ISOLATED_CASE").as_deref() == Ok(name.as_str()) {
        return Some(PathBuf::from(
            std::env::var_os("BASECRAFT_ISOLATED_ROOT").unwrap(),
        ));
    }
    let root = tempfile::tempdir().unwrap();
    let output = relaunch(&name)
        .env("BASECRAFT_ISOLATED_CASE", &name)
        .env("BASECRAFT_ISOLATED_ROOT", root.path())
        .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
        .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
        .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
        .env("TMPDIR", root.path())
        .env("TMP", root.path())
        .env("TEMP", root.path())
        .output()
        .unwrap();
    assert_one_passed(&name, &output);
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
        if $crate::isolated_profile::enter().is_none() {
            return;
        }
    };
}
#[allow(unused_imports)]
pub(crate) use isolate;
