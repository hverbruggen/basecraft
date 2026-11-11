//! Registry lookup keeps live alias protection without opening unrelated ledgers.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{analysis_store::ArtifactRun, input_evidence as evidence};
use std::path::Path;

fn reference(root: &Path, source: &Path) -> ArtifactRun {
    std::fs::create_dir_all(root).unwrap();
    evidence::reference_input_with_dependencies(
        &root.join("result.txt"),
        source,
        &basecraft_io::hash_file(source).unwrap(),
        &[],
        "index test",
    )
    .unwrap()
}

#[test]
fn external_replacement_and_new_alias_are_still_detected() {
    let Some(root) =
        isolated_profile::enter("external_replacement_and_new_alias_are_still_detected")
    else {
        return;
    };
    let source = root.join("source.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    let run = reference(&root.join("analysis"), &source);
    let replacement = root.join("replacement.fasta");
    std::fs::write(&replacement, b">a\nTTTT\n").unwrap();
    std::fs::rename(replacement, &source).unwrap();
    let alias = root.join("alias.fasta");
    std::fs::hard_link(&source, &alias).unwrap();
    let check = evidence::check(&alias).unwrap();
    assert_eq!(check.conflict.analyses.len(), 1);
    assert_eq!(check.conflict.analyses[0].run, run.directory());
}

#[test]
fn old_writer_ledger_replacement_is_reconciled() {
    let Some(root) = isolated_profile::enter("old_writer_ledger_replacement_is_reconciled") else {
        return;
    };
    let source = root.join("source1.fasta");
    let other = root.join("source2.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    std::fs::write(&other, b">a\nTTTT\n").unwrap();
    let run = reference(&root.join("analysis"), &source);
    assert!(
        evidence::check(&other)
            .unwrap()
            .conflict
            .analyses
            .is_empty()
    );
    let ledger = run.artifact("input.json").unwrap();
    let before = std::fs::metadata(&ledger).unwrap();
    let mut input = evidence::read_input(run.directory()).unwrap();
    let reference = input.source.as_mut().unwrap();
    reference.path = evidence::relative(run.directory(), &other).unwrap();
    reference.stamp = evidence::stamp(&other).unwrap();
    reference.hash = basecraft_io::hash_file(&other).unwrap();
    // Old in-place writer restores mtime and keeps size: ctime still changes.
    let mut bytes = serde_json::to_vec_pretty(&input).unwrap();
    bytes.push(b'\n');
    assert_eq!(bytes.len() as u64, before.len());
    std::fs::write(&ledger, bytes).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&ledger)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(before.modified().unwrap()))
        .unwrap();
    let check = evidence::check(&other).unwrap();
    assert!(check.is_complete());
    assert_eq!(check.conflict.analyses.len(), 1);
}

#[test]
fn corrupt_cache_cannot_hide_a_reference() {
    let Some(root) = isolated_profile::enter("corrupt_cache_cannot_hide_a_reference") else {
        return;
    };
    let source = root.join("source.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    reference(&root.join("analysis"), &source);
    let cache = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-input-index.json");
    std::fs::write(cache, b"broken cache").unwrap();
    assert_eq!(evidence::check(&source).unwrap().conflict.analyses.len(), 1);
}

#[cfg(feature = "test-counters")]
#[test]
fn two_hundred_stores_fifty_runs_open_no_unrelated_ledgers() {
    let Some(root) =
        isolated_profile::enter("two_hundred_stores_fifty_runs_open_no_unrelated_ledgers")
    else {
        return;
    };
    for store in 0..200 {
        let folder = root.join(format!("store-{store}"));
        std::fs::create_dir(&folder).unwrap();
        let source = folder.join("source.fasta");
        std::fs::write(&source, b">a\nACGT\n").unwrap();
        let first = reference(&folder, &source);
        let input = evidence::read_input(first.directory()).unwrap();
        let _lock = evidence::lock().unwrap();
        for run in 0..49 {
            let run =
                ArtifactRun::create(&folder.join("result.txt"), &format!("run-{run}")).unwrap();
            evidence::write_json(&run.artifact("input.json").unwrap(), &input).unwrap();
        }
        evidence::register(&first).unwrap();
    }
    let target = root.join("unrelated.fasta");
    std::fs::write(&target, b">b\nAAAA\n").unwrap();
    basecraft_io::counters::reset();
    let started = std::time::Instant::now();
    let check = evidence::check(&target).unwrap();
    let elapsed = started.elapsed();
    let counts = basecraft_io::counters::snapshot();
    assert!(check.is_complete());
    assert!(check.conflict.analyses.is_empty());
    assert!(
        counts.store_inspections.is_empty(),
        "{:?}",
        counts.store_inspections
    );
    assert!(
        counts
            .reads
            .keys()
            .all(|path| !path.starts_with(&root) || path.starts_with(root.join("profile"))),
        "{:?}",
        counts.reads
    );
    eprintln!(
        "200 stores x 50 runs: {elapsed:?}; complete file reads: {}; bytes: {}",
        counts.reads.values().map(|r| r.full_reads).sum::<u64>(),
        counts.reads.values().map(|r| r.bytes).sum::<u64>()
    );
    let mut session = basecraft_ops::Session::new(&root);
    let doc = session.open(&target).unwrap();
    session
        .apply(
            doc,
            basecraft_protocol::Op::SeqReplace {
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    basecraft_io::counters::reset();
    let started = std::time::Instant::now();
    session.save(doc, None).unwrap();
    let counts = basecraft_io::counters::snapshot();
    assert!(counts.store_inspections.is_empty());
    assert!(counts.reads.keys().all(|p| {
        !p.strip_prefix(&root)
            .ok()
            .and_then(|p| p.components().next())
            .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with("store-"))
    }));
    eprintln!(
        "200 stores x 50 runs Session save: {:?}; complete reads: {}; bytes: {}",
        started.elapsed(),
        counts.reads.values().map(|r| r.full_reads).sum::<u64>(),
        counts.reads.values().map(|r| r.bytes).sum::<u64>()
    );
}

#[test]
fn parseable_but_modified_cache_cannot_hide_a_reference() {
    let Some(root) =
        isolated_profile::enter("parseable_but_modified_cache_cannot_hide_a_reference")
    else {
        return;
    };
    let source = root.join("source.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    reference(&root.join("analysis"), &source);
    let cache = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-input-index.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap();
    for store in value["stores"].as_object_mut().unwrap().values_mut() {
        store["sources"] = serde_json::Value::Array(Vec::new());
        store["files"] = serde_json::Value::Object(Default::default());
    }
    std::fs::write(cache, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(evidence::check(&source).unwrap().conflict.analyses.len(), 1);
}

#[test]
fn stray_regular_file_does_not_make_an_unrelated_store_incomplete() {
    let Some(root) =
        isolated_profile::enter("stray_regular_file_does_not_make_an_unrelated_store_incomplete")
    else {
        return;
    };
    let source = root.join("source.fasta");
    let other = root.join("other.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    std::fs::write(&other, b">b\nAAAA\n").unwrap();
    let run = reference(&root.join("analysis"), &source);
    std::fs::write(
        run.directory().parent().unwrap().join("notes.txt"),
        b"notes",
    )
    .unwrap();
    let check = evidence::check(&other).unwrap();
    assert!(check.is_complete());
    assert!(check.conflict.analyses.is_empty());
    assert_eq!(evidence::check(&source).unwrap().conflict.analyses.len(), 1);
}

#[cfg(feature = "test-counters")]
#[test]
fn completed_registration_is_warm_and_cache_extensions_survive() {
    let Some(root) =
        isolated_profile::enter("completed_registration_is_warm_and_cache_extensions_survive")
    else {
        return;
    };
    let source = root.join("source.fasta");
    let other = root.join("other.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    std::fs::write(&other, b">b\nAAAA\n").unwrap();
    let run = reference(&root.join("analysis"), &source);
    basecraft_io::counters::reset();
    assert!(evidence::check(&other).unwrap().is_complete());
    assert!(
        basecraft_io::counters::snapshot()
            .reads
            .keys()
            .all(|p| !p.starts_with(root.join("analysis")))
    );
    let cache = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-input-index.json");
    let bytes = std::fs::read(&cache).unwrap();
    let mut record: basecraft_protocol::records::AnalysisInputIndex =
        serde_json::from_slice(&bytes).unwrap();
    record.extra.insert("extension".into(), true.into());
    for entry in record.stores.values_mut() {
        entry.extra.insert("extension".into(), true.into());
    }
    evidence::write_json(&cache, &record).unwrap();
    let _lock = evidence::lock().unwrap();
    evidence::register(&run).unwrap();
    let restored: basecraft_protocol::records::AnalysisInputIndex =
        serde_json::from_slice(&std::fs::read(cache).unwrap()).unwrap();
    assert_eq!(restored.extra, record.extra);
    assert!(
        restored
            .stores
            .values()
            .all(|s| s.extra.get("extension") == Some(&true.into()))
    );
}
