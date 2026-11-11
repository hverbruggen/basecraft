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
fn copied_identity_does_not_retire_the_live_original_store() {
    let Some(root) =
        isolated_profile::enter("copied_identity_does_not_retire_the_live_original_store")
    else {
        return;
    };
    let source = root.join("original.fasta");
    let other = root.join("copy.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    std::fs::write(&other, b">b\nTTTT\n").unwrap();
    let original = reference(&root.join("original"), &source);
    let copied = reference(&root.join("copied"), &other);
    let original_store = original.directory().parent().unwrap().parent().unwrap();
    let copied_store = copied.directory().parent().unwrap().parent().unwrap();
    std::fs::copy(
        original_store.join("identity"),
        copied_store.join("identity"),
    )
    .unwrap();
    evidence::register(&copied).unwrap();
    for path in [source, other] {
        let check = evidence::check(&path).unwrap();
        assert!(check.is_complete());
        assert_eq!(check.conflict.analyses.len(), 1);
    }
}

#[test]
fn corrupt_alias_shard_keeps_other_known_conflicts() {
    let Some(root) = isolated_profile::enter("corrupt_alias_shard_keeps_other_known_conflicts")
    else {
        return;
    };
    let source = root.join("source.fasta");
    let alias = root.join("alias.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    std::fs::hard_link(&source, &alias).unwrap();
    let known = reference(&root.join("known"), &source);
    reference(&root.join("other"), &alias);
    let state = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state;
    let key = basecraft_io::hash_bytes(alias.as_os_str().as_encoded_bytes());
    std::fs::write(
        state
            .join("analysis-inputs/sources")
            .join(key.trim_start_matches("b3:")),
        b"damaged",
    )
    .unwrap();
    let check = evidence::check(&source).unwrap();
    assert!(!check.is_complete());
    assert!(
        check
            .conflict
            .analyses
            .iter()
            .any(|analysis| analysis.run == known.directory())
    );
    let mut session = basecraft_ops::Session::new(&root);
    let doc = session.open(&source).unwrap();
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
    let decision = session
        .check_save(doc, None)
        .unwrap()
        .decide(evidence::SavePolicy::Cancel, true);
    assert!(session.save_with_decision(doc, None, decision).is_err());
    assert_eq!(std::fs::read(source).unwrap(), b">a\nACGT\n");
}

#[test]
fn current_writer_updates_source_index() {
    let Some(root) = isolated_profile::enter("current_writer_updates_source_index") else {
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
    let mut input = evidence::read_input(run.directory()).unwrap();
    let reference = input.source.as_mut().unwrap();
    reference.path = evidence::relative(run.directory(), &other).unwrap();
    reference.stamp = evidence::stamp(&other).unwrap();
    reference.hash = basecraft_io::hash_file(&other).unwrap();
    evidence::Mutation::begin()
        .unwrap()
        .write_input(run.directory(), &input)
        .unwrap();
    let check = evidence::check(&other).unwrap();
    assert!(check.is_complete());
    assert_eq!(check.conflict.analyses.len(), 1);
}

#[test]
fn failed_dependency_update_cannot_hide_a_later_relinked_source() {
    let Some(root) =
        isolated_profile::enter("failed_dependency_update_cannot_hide_a_later_relinked_source")
    else {
        return;
    };
    let a = root.join("a.fasta");
    let b = root.join("b.fasta");
    std::fs::write(&a, b">a\nACGT\n").unwrap();
    std::fs::write(&b, b">b\nTTTT\n").unwrap();
    let owner = reference(&root.join("owner"), &a);
    let dependent = reference(&root.join("dependent"), &a);
    let sibling = ArtifactRun::create(&root.join("dependent/result.txt"), "broken").unwrap();
    std::fs::write(sibling.directory().join("input.json"), b"damaged").unwrap();
    let mut input = evidence::read_input(dependent.directory()).unwrap();
    input.extra.insert(
        "upstream".into(),
        serde_json::json!([{
            "bundle": evidence::relative(dependent.directory(), owner.directory()).unwrap()
        }]),
    );
    evidence::Mutation::begin()
        .unwrap()
        .write_input(dependent.directory(), &input)
        .unwrap();
    let mut input = evidence::read_input(owner.directory()).unwrap();
    let source = input.source.as_mut().unwrap();
    source.path = evidence::relative(owner.directory(), &b).unwrap();
    source.hash = basecraft_io::hash_file(&b).unwrap();
    source.stamp = evidence::stamp(&b).unwrap();
    evidence::Mutation::begin()
        .unwrap()
        .write_input(owner.directory(), &input)
        .unwrap();
    let check = evidence::check(&b).unwrap();
    assert!(
        !check.is_complete(),
        "unpublished ancestry must remain uncertain"
    );
    assert!(
        check
            .conflict
            .analyses
            .iter()
            .any(|analysis| analysis.run == owner.directory())
    );
}

#[test]
fn damaged_catalogue_cannot_hide_a_reference() {
    let Some(root) = isolated_profile::enter("damaged_catalogue_cannot_hide_a_reference") else {
        return;
    };
    let source = root.join("source.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    let run = reference(&root.join("analysis"), &source);
    let cache = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-input-index.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap();
    value["value"]["sources"] = serde_json::Value::Array(Vec::new());
    for damaged in [serde_json::to_vec(&value).unwrap(), b"broken cache".to_vec()] {
        std::fs::write(&cache, damaged).unwrap();
        let check = evidence::check(&source).unwrap();
        assert_eq!(check.conflict.analyses.len(), 1);
        assert_eq!(check.conflict.analyses[0].run, run.directory());
    }
}

#[test]
fn deleted_project_does_not_concern_other_saves() {
    let Some(root) = isolated_profile::enter("deleted_project_does_not_concern_other_saves")
    else {
        return;
    };
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    let source = project.join("source.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    reference(&project.join("analysis"), &source);
    std::fs::remove_dir_all(&project).unwrap();
    let other = root.join("other.fasta");
    std::fs::write(&other, b">b\nAAAA\n").unwrap();
    let check = evidence::check(&other).unwrap();
    assert!(check.is_complete(), "{:?}", check.issues);
    assert!(check.conflict.analyses.is_empty());
}

#[cfg(feature = "test-counters")]
#[test]
fn two_hundred_stores_fifty_runs_open_no_unrelated_ledgers() {
    let Some(root) =
        isolated_profile::enter("two_hundred_stores_fifty_runs_open_no_unrelated_ledgers")
    else {
        return;
    };
    if std::env::var_os("BASECRAFT_INDEX_COLD").is_some() {
        measure_save(&root, "cold", "G", false);
        return;
    }
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
            // Build a complete current-format fixture, then publish its index once.
            std::fs::write(
                run.artifact("input.json").unwrap(),
                serde_json::to_vec(&input).unwrap(),
            )
            .unwrap();
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
    measure_save(&root, "warm", "T", false);
    for (label, residue) in [("repeat-1", "G"), ("repeat-2", "T"), ("repeat-3", "G")] {
        measure_save(&root, label, residue, false);
    }
    for (label, residue) in [
        ("interactive-1", "T"),
        ("interactive-2", "G"),
        ("interactive-3", "T"),
    ] {
        measure_save(&root, label, residue, true);
    }
    // Minimal metadata-only work, with paths already decoded in memory.
    let cache = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state
        .join("analysis-input-index.json");
    let index: basecraft_protocol::records::IndexFile<
        basecraft_protocol::records::AnalysisInputIndex,
    > = serde_json::from_slice(&std::fs::read(cache).unwrap()).unwrap();
    let paths: Vec<_> = index.value.sources.iter().collect();
    let started = std::time::Instant::now();
    for path in &paths {
        let _ = std::fs::symlink_metadata(path);
    }
    eprintln!(
        "{} cached metadata paths, stat only: {:?}",
        paths.len(),
        started.elapsed()
    );
    let cold = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "two_hundred_stores_fifty_runs_open_no_unrelated_ledgers",
            "--nocapture",
        ])
        .env("BASECRAFT_INDEX_COLD", "1")
        .output()
        .unwrap();
    eprint!(
        "{}{}",
        String::from_utf8_lossy(&cold.stdout),
        String::from_utf8_lossy(&cold.stderr)
    );
    assert!(cold.status.success());
}

#[cfg(feature = "test-counters")]
fn measure_save(root: &Path, label: &str, residue: &str, interactive: bool) {
    // Interactive sessions journal each edit immediately; Session::new batches
    // typing until save. Measure both without moving work out of the timed call.
    let mut session = if interactive {
        basecraft_ops::Session::recoverable(root).unwrap()
    } else {
        basecraft_ops::Session::new(root)
    };
    let doc = session.open(&root.join("unrelated.fasta")).unwrap();
    session
        .apply(
            doc,
            basecraft_protocol::Op::SeqReplace {
                start: 0,
                end: 1,
                residues: residue.into(),
            },
        )
        .unwrap();
    basecraft_io::counters::reset();
    let started = std::time::Instant::now();
    session.save(doc, None).unwrap();
    let elapsed = started.elapsed();
    let counts = basecraft_io::counters::snapshot();
    assert!(counts.store_inspections.is_empty());
    assert!(counts.reads.values().map(|r| r.bytes).sum::<u64>() < 100_000);
    assert!(counts.reads.keys().all(|p| {
        !p.strip_prefix(root)
            .ok()
            .and_then(|p| p.components().next())
            .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with("store-"))
    }));
    eprintln!(
        "200 stores x 50 runs {label} Session save: {:?}; complete reads: {}; bytes: {}",
        elapsed,
        counts.reads.values().map(|r| r.full_reads).sum::<u64>(),
        counts.reads.values().map(|r| r.bytes).sum::<u64>()
    );
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
fn completed_registration_needs_no_unrelated_ledger_reads() {
    let Some(root) =
        isolated_profile::enter("completed_registration_needs_no_unrelated_ledger_reads")
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
    let record: basecraft_protocol::records::IndexFile<
        basecraft_protocol::records::AnalysisInputIndex,
    > = serde_json::from_slice(&bytes).unwrap();
    let _lock = evidence::lock().unwrap();
    evidence::register(&run).unwrap();
    let restored: basecraft_protocol::records::IndexFile<
        basecraft_protocol::records::AnalysisInputIndex,
    > = serde_json::from_slice(&std::fs::read(cache).unwrap()).unwrap();
    assert_eq!(restored.value.sources, record.value.sources);
    assert_eq!(restored.value.stores, record.value.stores);
}
