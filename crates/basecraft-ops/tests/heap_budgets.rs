#![cfg(feature = "test-heap")]
#[path = "../benches/interaction/fixtures.rs"]
mod fixtures;
#[path = "../benches/interaction/memory_cases.rs"]
mod memory_cases;

#[global_allocator]
static ALLOCATOR: basecraft_io::heap::Allocator = basecraft_io::heap::Allocator;

#[test]
fn recoverable_open_requested_heap() {
    if std::env::var_os("BASECRAFT_HEAP_CHILD").is_none() {
        let root = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recoverable_open_requested_heap", "--nocapture"])
            .env("BASECRAFT_HEAP_CHILD", root.path())
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
            .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
            .env("BASECRAFT_SCRATCH_DIR", root.path())
            .env("TMPDIR", root.path())
            .output()
            .unwrap();
        println!("{}", String::from_utf8_lossy(&output.stdout));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("BASECRAFT_HEAP_CHILD").unwrap());
    for scenario in ["heap-open-compact", "heap-open-100mb", "heap-open-error"] {
        let result = memory_cases::measure(scenario, &root).unwrap();
        println!("{result}");
        // Threshold follows measured release/debug baselines; selected after capture.
        let heap = &result["heap"];
        assert!(
            heap["peak_bytes"].as_u64().unwrap() >= heap["live_after_open_bytes"].as_u64().unwrap()
        );
    }
}
