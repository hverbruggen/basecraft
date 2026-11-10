//! Requested heap only; fixture/profile/Session setup and verification are untimed.
use basecraft_io::heap::{self, Measurement};
use basecraft_ops::Session;
use basecraft_platform::paths::AppPaths;
use serde_json::{Value, json};
use std::{error::Error, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub fn measure(scenario: &str, root: &Path) -> Result<Value> {
    let bytes = match scenario {
        "heap-open-compact" => 4096,
        "heap-open-100mb" => 100_000_000,
        "heap-open-error" => 4096,
        _ => return Err("unknown heap scenario".into()),
    };
    let paths = AppPaths::discover()?;
    if !root.is_absolute() || paths.config != root.join("profile/config") {
        return Err("heap measurement requires an isolated absolute profile".into());
    }
    let file = root.join("input.fasta");
    super::fixtures::fixture(&file, Some(bytes))?;
    if scenario == "heap-open-error" {
        // Recognized FASTA suffix, genuinely malformed input, real loader failure.
        std::fs::write(&file, b"not a FASTA header\nACGT\n")?;
    }
    let fixture_bytes = file.metadata()?.len();
    if scenario != "heap-open-error" {
        let mut warm = Session::recoverable_with_paths(root, &paths)?;
        warm.open(&file)?;
    }
    let mut session = Session::recoverable_with_paths(root, &paths)?;
    let measurement = Measurement::start()?;
    // Neither fixture generation nor Session/profile construction belongs to open.
    let opened = session.open(&file);
    let stats = measurement.finish()?;
    if stats.failed_allocations != 0 || stats.failed_reallocations != 0 {
        return Err("allocator failure during measured open".into());
    }
    if scenario == "heap-open-error" {
        if opened.is_ok() {
            return Err("malformed FASTA unexpectedly opened".into());
        }
    } else {
        let id = opened?;
        // Checked public API; never introduce dependencies on panicking getters.
        let doc = session.checked_sequence(id)?;
        let payload = bytes - 7;
        let len = payload - payload / 81;
        if doc.len() != len {
            return Err("heap fixture length mismatch".into());
        }
        for offset in (0..len).step_by(65536) {
            let residues = doc.read(offset, (offset + 65536).min(len));
            if residues
                .iter()
                .enumerate()
                .any(|(i, b)| *b != b"ACGT"[((offset + i as u64) % 4) as usize])
            {
                return Err("heap fixture payload mismatch".into());
            }
        }
    }
    // Error result and verification temporaries are gone before Session teardown.
    drop(session);
    let after_drop = heap::live_bytes()?;
    Ok(json!({
        "schema_version": 2, "scenario": scenario, "verified": true,
        "source_commit": option_env!("BASECRAFT_BENCH_REVISION").unwrap_or("unknown"),
        "debug_assertions": cfg!(debug_assertions),
        "fixture_bytes": fixture_bytes, "output_bytes": file.metadata()?.len(),
        "operations": 1, "rows": 1, "columns": null, "edits": 0,
        "work": {"instrumentation": heap::INSTRUMENTATION, "unit": "requested bytes",
                 "window": "Session::open", "recovery": "default", "warmup": scenario != "heap-open-error",
                 "fixture": "single-record-wrapped-ACGT-v1", "expected_error": scenario == "heap-open-error"},
        "heap": {"baseline_bytes": stats.baseline_bytes, "peak_bytes": stats.peak_bytes,
                 "incremental_peak_bytes": stats.incremental_peak_bytes, "live_after_open_bytes": stats.live_bytes,
                 "live_after_drop_bytes": after_drop,
                 "successful_reallocations": stats.successful_reallocations,
                 "failed_reallocations": stats.failed_reallocations, "failed_allocations": stats.failed_allocations}
    }))
}
