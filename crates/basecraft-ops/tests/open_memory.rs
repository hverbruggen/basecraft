//! Every editable kind: exact source reads and measured requested heap.
#![cfg(all(feature = "test-counters", feature = "test-heap"))]
use basecraft_io::{counters, heap};
use basecraft_ops::{Session, files::Kind};
use std::path::{Path, PathBuf};

#[global_allocator]
static ALLOCATOR: heap::Allocator = heap::Allocator;

fn copy(root: &Path, fixture: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/compact")
        .join(fixture);
    let target = root.join(source.file_name().unwrap());
    std::fs::copy(source, &target).unwrap();
    target
}

fn files(root: &Path) -> std::collections::BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut result = std::collections::BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                result.insert(path.clone(), None);
                result.extend(files(&path));
            } else {
                result.insert(path.clone(), Some(std::fs::read(path).unwrap()));
            }
        }
    }
    result
}

#[test]
fn every_kind_reads_each_input_once() {
    let Ok(case) = std::env::var("BASECRAFT_OPEN_CASE") else {
        for case in [
            "sequence-fasta",
            "sequence-genbank",
            "alignment",
            "tree",
            "delimitation",
            "metadata",
            "submission",
        ] {
            let root = tempfile::tempdir().unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "every_kind_reads_each_input_once", "--nocapture"])
                .env("BASECRAFT_OPEN_CASE", case)
                .env("BASECRAFT_OPEN_ROOT", root.path())
                .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
                .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
                .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
                .env("BASECRAFT_SCRATCH_DIR", root.path())
                .output()
                .unwrap();
            println!("{}", String::from_utf8_lossy(&result.stdout));
            assert!(
                result.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        return;
    };
    let root = PathBuf::from(std::env::var_os("BASECRAFT_OPEN_ROOT").unwrap());
    let (file, kind, input_count) = match case.as_str() {
        "sequence-fasta" => {
            copy(&root, "sequences-annotations/locus.gff3");
            (
                copy(&root, "sequences-annotations/locus.fasta"),
                Kind::Sequence,
                2,
            )
        }
        "sequence-genbank" => (
            copy(&root, "sequences-annotations/locus.gb"),
            Kind::Sequence,
            1,
        ),
        "alignment" => (
            copy(&root, "alignment-editing/ambiguous-gappy.afa"),
            Kind::Alignment,
            1,
        ),
        "tree" => (copy(&root, "tree-inference/unrooted.nwk"), Kind::Tree, 2),
        "delimitation" => (
            copy(&root, "species-delimitation/hypotheses.spart"),
            Kind::Delimitation,
            2,
        ),
        "metadata" => (
            basecraft_ops::metadata::create(&root).unwrap(),
            Kind::Metadata,
            5,
        ),
        "submission" => {
            let alignment = copy(&root, "alignment-editing/ambiguous-gappy.afa");
            let file = root.join("review.submission.json");
            basecraft_ops::submission::create(&mut Session::unlogged(&root), &file, &alignment)
                .unwrap();
            (file, Kind::Submission, 1)
        }
        _ => panic!("unknown case"),
    };
    if matches!(kind, Kind::Tree | Kind::Delimitation) {
        let mut writer = Session::unlogged(&root);
        let doc = writer.open(&file).unwrap();
        writer.save(doc, None).unwrap();
    }
    // Warm only discovery and caches. Fixture generation and Session setup are
    // excluded from open's allocation window, as in the large-file heap budget.
    Session::recoverable(&root).unwrap().open(&file).unwrap();
    let before = files(&root);
    let mut session = Session::recoverable(&root).unwrap();
    counters::reset();
    let measurement = heap::Measurement::start().unwrap();
    let (doc, actual) = session.open_classified(&file).unwrap();
    let memory = measurement.finish().unwrap();
    let reads = counters::snapshot();
    assert_eq!(actual, kind);
    assert_eq!(reads.unresolved_reads, 0);
    assert_eq!((reads.sync_all, reads.sync_data), (0, 0));
    assert_eq!(reads.reads.len(), input_count);
    let mut bytes_read = 0;
    for (path, count) in &reads.reads {
        assert_eq!(count.full_reads, 1, "{}", path.display());
        assert_eq!(count.bytes, path.metadata().unwrap().len());
        bytes_read += count.bytes;
    }
    assert!(!session.document_record(doc).unwrap().unsaved);
    assert_eq!(memory.failed_allocations + memory.failed_reallocations, 0);
    println!(
        "OPEN {case}: files={input_count} bytes_read={bytes_read} peak_extra_bytes={} live_extra_bytes={}",
        memory.incremental_peak_bytes,
        memory.live_bytes - memory.baseline_bytes
    );
    assert_eq!(
        files(&root),
        before,
        "open must not write source, state or cache"
    );
    std::mem::forget(session); // A crash runs no Session destructor.
    assert_eq!(
        files(&root),
        before,
        "an open-only crash must leave no recovery folder"
    );
    // Same gross bound as the 100 MB test: one input copy plus decoded objects,
    // counters and fixed metadata. No second saved-form or recovery buffer.
    assert!(u64::try_from(memory.incremental_peak_bytes).unwrap() <= bytes_read * 2 + 65_536);
}
