#![cfg(feature = "test-heap")]
#[path = "../benches/interaction/fixtures.rs"]
mod fixtures;
#[path = "../benches/interaction/memory_cases.rs"]
mod memory_cases;

#[global_allocator]
static ALLOCATOR: basecraft_io::heap::Allocator = basecraft_io::heap::Allocator;

#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
#[test]
fn recoverable_open_requested_heap() {
    crate::isolated_profile::isolate!();
    if std::env::var_os("BASECRAFT_HEAP_CHILD").is_none() {
        let root = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recoverable_open_requested_heap", "--nocapture"])
            .env("BASECRAFT_HEAP_CHILD", root.path())
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
            .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
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
        // Measured at 39dae81 (debug + five independent release processes).
        // 100MB incremental peak ~100.986MB; compact ~10.8KB. This gross bound
        // permits almost 100% large-input headroom plus a fixed small-work floor.
        // It measures requested Rust heap, never RSS or allocator overhead.
        let heap = &result["heap"];
        let baseline = heap["baseline_bytes"].as_u64().unwrap();
        let peak = heap["peak_bytes"].as_u64().unwrap();
        let incremental = heap["incremental_peak_bytes"].as_u64().unwrap();
        let after_drop = heap["live_after_drop_bytes"].as_u64().unwrap();
        let limit = result["fixture_bytes"].as_u64().unwrap() * 2 + 65_536;
        assert_eq!(peak - baseline, incremental);
        assert!(peak >= heap["live_after_open_bytes"].as_u64().unwrap());
        assert!(
            incremental <= limit,
            "{scenario}: incremental peak {incremental} requested bytes exceeds {limit}"
        );
        assert!(
            after_drop <= baseline + 65_536,
            "{scenario}: live requested bytes after drop {after_drop} exceed baseline {baseline} + 65536"
        );
    }
}
