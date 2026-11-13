//! Sets `BASECRAFT_VERSION`, the version every binary reports and every
//! record carries: the workspace version, and on a nightly build its date
//! and commit as build metadata, `0.5.0+20261113.1a2b3c4d5`. A nightly build
//! sets `BASECRAFT_BUILD=<yyyymmdd>.<commit>`; other builds leave it unset.
fn main() {
    println!("cargo::rerun-if-env-changed=BASECRAFT_BUILD");
    let mut version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets the package version");
    let build = std::env::var("BASECRAFT_BUILD").unwrap_or_default();
    if !build.is_empty() {
        let fits = build.split_once('.').is_some_and(|(date, commit)| {
            date.len() == 8
                && date.bytes().all(|b| b.is_ascii_digit())
                && (7..=40).contains(&commit.len())
                && commit.bytes().all(|b| b.is_ascii_hexdigit())
        });
        assert!(
            fits,
            "BASECRAFT_BUILD must be <yyyymmdd>.<commit>, such as 20261113.1a2b3c4d5, not {build:?}"
        );
        version = format!("{version}+{build}");
    }
    println!("cargo::rustc-env=BASECRAFT_VERSION={version}");
}
