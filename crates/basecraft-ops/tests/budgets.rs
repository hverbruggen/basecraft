#![cfg(feature = "test-counters")]

use basecraft_io::counters;
use basecraft_ops::Session;
use basecraft_platform::paths::AppPaths;
use basecraft_protocol::Op;
use std::path::Path;

// Baseline: 2026-11-10; owned-source open counts measured against ab006f5.
// Counts describe current behavior, not a performance target or elapsed time.
// Each child owns a profile: the save evidence lock discovers AppPaths directly,
// independently of the paths supplied to Session. Do not mutate process env in
// the parallel test harness.
fn scenario(
    name: &str,
    fixture: &str,
    open_reads: u64,
    saved_reads: u64,
    save_sync_all: u64,
    edit: impl Fn(u32) -> Op,
) {
    let Some(root) = child_profile(name) else {
        return;
    };
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/compact")
        .join(fixture);
    let file = root.join(fixture.file_name().unwrap());
    std::fs::copy(fixture, &file).unwrap();
    let paths = AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| root.join("profile").into_os_string())
    })
    .unwrap();
    assert_eq!(AppPaths::discover().unwrap().state, paths.state);
    let original_size = file.metadata().unwrap().len();
    counters::reset();
    // Main.qml openPath resolves file_kind once and reuses it for routing
    // and the tab row. Its FASTA probe remains a separate read; Session now
    // hashes and parses one owned main-file read when companions are absent.
    basecraft_ops::files::kind(&file).unwrap();
    let mut session = Session::recoverable_with_paths(&root, &paths).unwrap();
    let doc = session.open(&file).unwrap();
    record(
        name,
        "open",
        &file,
        (open_reads, open_reads * original_size, 0, 0),
    );
    for i in 0..3 {
        counters::reset();
        session.apply(doc, edit(i)).unwrap();
        let expected = if i == 0 {
            (2, 2 * original_size, 2, 2)
        } else {
            (0, 0, 1, 0)
        };
        record(name, &format!("edit {}", i + 1), &file, expected);
    }
    counters::reset();
    session.save(doc, None).unwrap();
    // Saving reads twice before replacement and then retains the new bytes.
    // Tree sidecar serialization also hashes the newly written tree once.
    let saved_size = file.metadata().unwrap().len();
    record(
        name,
        "save",
        &file,
        (
            2 + saved_reads,
            2 * original_size + saved_reads * saved_size,
            1,
            save_sync_all,
        ),
    );
}

fn record(name: &str, stage: &str, file: &Path, expected: (u64, u64, u64, u64)) {
    let counts = counters::snapshot();
    let reads = counts
        .reads
        .get(&file.canonicalize().unwrap())
        .copied()
        .unwrap_or_default();
    assert_eq!(counts.unresolved_reads, 0);
    assert!(
        counts.store_inspections.is_empty(),
        "{name} {stage}: no stores registered"
    );
    assert_eq!(
        (
            reads.full_reads,
            reads.bytes,
            counts.sync_data,
            counts.sync_all
        ),
        expected,
        "{name} {stage}: full reads, bytes, sync_data, sync_all"
    );
    println!(
        "{name} {stage}: reads={} bytes={} size={} sync_data={} sync_all={}",
        reads.full_reads,
        reads.bytes,
        file.metadata().unwrap().len(),
        counts.sync_data,
        counts.sync_all
    );
}

#[test]
fn fasta() {
    scenario(
        "fasta",
        "alignment-editing/ambiguous-gappy.afa",
        2,
        1,
        12,
        |i| Op::AlnReplace {
            row: 0,
            start: u64::from(i),
            end: u64::from(i) + 1,
            residues: "C".into(),
        },
    );
}

#[test]
fn genbank() {
    scenario("genbank", "sequences-annotations/locus.gb", 1, 1, 12, |i| {
        Op::SeqReplace {
            start: u64::from(i),
            end: u64::from(i) + 1,
            residues: "C".into(),
        }
    });
}

#[test]
fn newick() {
    scenario("newick", "tree-inference/unrooted.nwk", 1, 2, 14, |i| {
        Op::TreeRenameTip {
            from: if i == 0 { "a".into() } else { format!("a{i}") },
            to: format!("a{}", i + 1),
        }
    });
}

#[path = "budgets/store_budgets.rs"]
mod store_budgets;

fn child_profile(name: &str) -> Option<std::path::PathBuf> {
    if std::env::var("BASECRAFT_BUDGET_CHILD").as_deref() != Ok(name) {
        let dir = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("BASECRAFT_BUDGET_CHILD", name)
            .env("BASECRAFT_BUDGET_ROOT", dir.path())
            .env("BASECRAFT_PROFILE_DIR", dir.path().join("profile"))
            .env("BASECRAFT_STATE_DIR", dir.path().join("profile/state"))
            .env("BASECRAFT_CACHE_DIR", dir.path().join("profile/cache"))
            .env("BASECRAFT_SCRATCH_DIR", dir.path())
            .env("TMPDIR", dir.path())
            .env("TMP", dir.path())
            .env("TEMP", dir.path())
            .output()
            .unwrap();
        println!("{}", String::from_utf8_lossy(&output.stdout));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }
    Some(std::path::PathBuf::from(
        std::env::var_os("BASECRAFT_BUDGET_ROOT").unwrap(),
    ))
}
