//! Registry lookup keeps live alias protection without opening unrelated ledgers.
use crate::isolated_profile;
use basecraft_ops::{records, records::ArtifactRun};
use std::path::Path;

fn reference(root: &Path, source: &Path) -> ArtifactRun {
    std::fs::create_dir_all(root).unwrap();
    records::reference_input(
        &root.join("result.txt"),
        source,
        &basecraft_io::hash_file(source).unwrap(),
        None,
        &[],
        "index test",
        None,
    )
    .unwrap()
}

#[test]
fn external_replacement_and_new_alias_are_still_detected() {
    let Some(root) = isolated_profile::enter() else {
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
    let check = records::check(&alias).unwrap();
    assert_eq!(check.conflict.analyses.len(), 1);
    assert_eq!(check.conflict.analyses[0].run, run.directory());
}

#[test]
fn copied_identity_does_not_retire_the_live_original_store() {
    let Some(root) = isolated_profile::enter() else {
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
    records::register(&copied).unwrap();
    for path in [source, other] {
        let check = records::check(&path).unwrap();
        assert!(check.is_complete());
        assert_eq!(check.conflict.analyses.len(), 1);
    }
}

#[test]
fn damaged_index_cannot_hide_a_reference() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let source = root.join("source.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    reference(&root.join("known"), &source);
    for bucket in std::fs::read_dir(root.join(".basecraft/analysis-index")).unwrap() {
        for entry in std::fs::read_dir(bucket.unwrap().path()).unwrap() {
            std::fs::write(entry.unwrap().path(), b"damaged").unwrap();
        }
    }
    assert!(!records::check(&source).unwrap().is_complete());
    let mut session = basecraft_ops::Session::new(&root);
    let doc = session.open(&source).unwrap();
    session
        .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
        .unwrap();
    assert!(session.save(doc, None).is_err());
    let decision = session
        .check_save(doc, None)
        .unwrap()
        .decide(records::SavePolicy::Cancel, false);
    assert!(session.save_with_decision(doc, None, decision).is_err());
    assert_eq!(std::fs::read(source).unwrap(), b">a\nACGT\n");
}

#[test]
fn current_writer_updates_source_index() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let source = root.join("source1.fasta");
    let other = root.join("source2.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    std::fs::write(&other, b">a\nTTTT\n").unwrap();
    let run = reference(&root.join("analysis"), &source);
    assert!(records::check(&other).unwrap().conflict.analyses.is_empty());
    let mut input = records::read_input(run.directory()).unwrap();
    let reference = input.source.as_mut().unwrap();
    reference.path = records::relative(run.directory(), &other).unwrap();
    reference.stamp = records::stamp(&other).unwrap();
    reference.hash = basecraft_io::hash_file(&other).unwrap();
    records::write_input(run.directory(), &input).unwrap();
    let check = records::check(&other).unwrap();
    assert!(check.is_complete());
    assert_eq!(check.conflict.analyses.len(), 1);
}

#[test]
fn failed_dependency_update_cannot_hide_a_later_relinked_source() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let a = root.join("a.fasta");
    let b = root.join("b.fasta");
    std::fs::write(&a, b">a\nACGT\n").unwrap();
    std::fs::write(&b, b">b\nTTTT\n").unwrap();
    let owner = reference(&root.join("owner"), &a);
    let dependent = reference(&root.join("dependent"), &a);
    let sibling = ArtifactRun::create(None, &root.join("dependent/result.txt"), "broken").unwrap();
    std::fs::write(sibling.directory().join("input.json"), b"damaged").unwrap();
    let mut input = records::read_input(dependent.directory()).unwrap();
    input.extra.insert(
        "upstream".into(),
        serde_json::json!([{
            "bundle": records::relative(dependent.directory(), owner.directory()).unwrap()
        }]),
    );
    records::write_input(dependent.directory(), &input).unwrap();
    let mut input = records::read_input(owner.directory()).unwrap();
    let source = input.source.as_mut().unwrap();
    source.path = records::relative(owner.directory(), &b).unwrap();
    source.hash = basecraft_io::hash_file(&b).unwrap();
    source.stamp = records::stamp(&b).unwrap();
    records::write_input(owner.directory(), &input).unwrap();
    let check = records::check(&b).unwrap();
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
fn deleted_project_does_not_concern_other_saves() {
    let Some(root) = isolated_profile::enter() else {
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
    let check = records::check(&other).unwrap();
    assert!(check.is_complete(), "{:?}", check.issues);
    assert!(check.conflict.analyses.is_empty());
}

#[cfg(feature = "test-counters")]
#[test]
fn two_hundred_stores_fifty_runs_open_no_unrelated_ledgers() {
    let Some(root) = isolated_profile::enter() else {
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
        let input = records::read_input(first.directory()).unwrap();
        let _lock = records::lock().unwrap();
        for run in 0..49 {
            let run = ArtifactRun::create(None, &folder.join("result.txt"), &format!("run-{run}"))
                .unwrap();
            // Build a complete current-format fixture, then publish its index once.
            std::fs::write(
                run.artifact("input.json").unwrap(),
                serde_json::to_vec(&input).unwrap(),
            )
            .unwrap();
        }
        records::register(&first).unwrap();
    }
    let target = root.join("unrelated.fasta");
    std::fs::write(&target, b">b\nAAAA\n").unwrap();
    basecraft_io::counters::reset();
    let started = std::time::Instant::now();
    let check = records::check(&target).unwrap();
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
    let cold = crate::isolated_profile::relaunch(&crate::isolated_profile::test_name())
        .env("BASECRAFT_INDEX_COLD", "1")
        .output()
        .unwrap();
    eprint!(
        "{}{}",
        String::from_utf8_lossy(&cold.stdout),
        String::from_utf8_lossy(&cold.stderr)
    );
    crate::isolated_profile::assert_one_passed(&crate::isolated_profile::test_name(), &cold);
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
        .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, residue))
        .unwrap();
    // Recovery sessions retain the original by hard link at the first edit,
    // so the save also covers the multi-link lookup.
    let links = basecraft_platform::files::link_count(&root.join("unrelated.fasta")).unwrap();
    basecraft_io::counters::reset();
    let started = std::time::Instant::now();
    session.save(doc, None).unwrap();
    let elapsed = started.elapsed();
    let counts = basecraft_io::counters::snapshot();
    assert!(counts.store_inspections.is_empty());
    // Includes index shards and, for a multi-link target, the catalogue.
    assert!(counts.reads.values().map(|r| r.bytes).sum::<u64>() < 100_000);
    assert!(counts.reads.keys().all(|p| {
        !p.strip_prefix(root)
            .ok()
            .and_then(|p| p.components().next())
            .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with("store-"))
    }));
    eprintln!(
        "200 stores x 50 runs {label} Session save: {:?}; links: {links}; complete reads: {}; bytes: {}",
        elapsed,
        counts.reads.values().map(|r| r.full_reads).sum::<u64>(),
        counts.reads.values().map(|r| r.bytes).sum::<u64>()
    );
}

#[test]
fn stray_regular_file_does_not_make_an_unrelated_store_incomplete() {
    let Some(root) = isolated_profile::enter() else {
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
    let check = records::check(&other).unwrap();
    assert!(check.is_complete());
    assert!(check.conflict.analyses.is_empty());
    assert_eq!(records::check(&source).unwrap().conflict.analyses.len(), 1);
}

#[cfg(feature = "test-counters")]
#[test]
fn completed_registration_needs_no_unrelated_ledger_reads() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let source = root.join("source.fasta");
    let other = root.join("other.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    std::fs::write(&other, b">b\nAAAA\n").unwrap();
    let run = reference(&root.join("analysis"), &source);
    basecraft_io::counters::reset();
    assert!(records::check(&other).unwrap().is_complete());
    assert!(
        basecraft_io::counters::snapshot()
            .reads
            .keys()
            .all(|p| !p.starts_with(root.join("analysis")))
    );
    // Registering again changes nothing.
    let index = std::fs::read_dir(root.join(".basecraft/analysis-index"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let shard = std::fs::read_dir(&index)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = std::fs::read(&shard).unwrap();
    let _lock = records::lock().unwrap();
    records::register(&run).unwrap();
    assert_eq!(std::fs::read(shard).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(index).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn symlinked_folder_spelling_and_new_inode_are_still_detected() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let real = root.join("disk/data");
    std::fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, root.join("data")).unwrap();
    let recorded = root.join("data/source.fasta");
    std::fs::write(&recorded, b">a\nACGT\n").unwrap();
    let run = reference(&root.join("analysis"), &recorded);
    // A sync tool restores the same bytes as a new file.
    let real_file = real.join("source.fasta");
    let copy = real.join("restored.tmp");
    std::fs::copy(&real_file, &copy).unwrap();
    std::fs::rename(&copy, &real_file).unwrap();
    let check = records::check(&real_file).unwrap();
    assert_eq!(check.conflict.analyses.len(), 1);
    assert_eq!(check.conflict.analyses[0].run, run.directory());
}

#[test]
fn private_and_vanished_stores_do_not_concern_other_saves() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    // Recovery staging inside the profile's state folder is never indexed.
    let state = basecraft_platform::paths::AppPaths::discover()
        .unwrap()
        .state;
    let staging = state.join("recovery/session/.basecraft/objects/stage");
    std::fs::create_dir_all(&staging).unwrap();
    let staged = staging.join("tree.fasta");
    std::fs::write(&staged, b">a\nACGT\n").unwrap();
    reference(&staging, &staged);
    assert!(!staging.join(".basecraft/analysis-index").exists());
    // A store deleted with its folder.
    let project = root.join("gone");
    std::fs::create_dir(&project).unwrap();
    let source = project.join("source.fasta");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    reference(&project.join("results"), &source);
    std::fs::remove_dir_all(project.join("results")).unwrap();
    std::fs::remove_dir_all(state.join("recovery")).unwrap();
    for target in [root.join("other.fasta"), source] {
        std::fs::write(&target, b">b\nAAAA\n").unwrap();
        let check = records::check(&target).unwrap();
        assert!(check.is_complete(), "{:?}", check.issues);
        assert!(check.conflict.analyses.is_empty());
    }
}

#[cfg(feature = "test-counters")]
#[test]
fn a_save_in_one_project_leaves_another_alone() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let mut sources = Vec::new();
    for name in ["first", "second"] {
        let project = root.join(name);
        std::fs::create_dir_all(project.join("results")).unwrap();
        let source = project.join("reads.fasta");
        std::fs::write(&source, b">a\nACGT\n").unwrap();
        reference(&project.join("results"), &source);
        sources.push(source);
    }
    let second = root.join("second");
    let before = tree(&second);
    // A file in the first project with the same name as the second's input.
    let target = root.join("first/results/reads.fasta");
    std::fs::write(&target, b">b\nAAAA\n").unwrap();
    let mut session = basecraft_ops::Session::new(root.join("first"));
    let doc = session.open(&target).unwrap();
    session
        .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
        .unwrap();
    basecraft_io::counters::reset();
    session.save(doc, None).unwrap();
    let counts = basecraft_io::counters::snapshot();
    assert!(counts.store_inspections.is_empty());
    assert!(counts.reads.keys().all(|p| !p.starts_with(&second)));
    assert_eq!(tree(&second), before);
    // The first project's own input is still protected.
    assert_eq!(
        records::check(&sources[0]).unwrap().conflict.analyses.len(),
        1
    );
}

/// Every file below `folder` with its bytes.
#[cfg(feature = "test-counters")]
fn tree(folder: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap().flatten() {
        if entry.file_type().unwrap().is_dir() {
            files.extend(tree(&entry.path()));
        } else {
            files.push((entry.path(), std::fs::read(entry.path()).unwrap()));
        }
    }
    files.sort();
    files
}

#[cfg(feature = "test-counters")]
#[test]
fn a_project_save_reads_one_bucket_of_its_index() {
    let Some(root) = isolated_profile::enter() else {
        return;
    };
    let folder = root.join("project");
    std::fs::create_dir_all(folder.join("data")).unwrap();
    let project = basecraft_ops::project::Project::new(&folder).unwrap();
    for i in 0..300 {
        let results = folder.join(format!("results-{}", i % 30));
        std::fs::create_dir_all(&results).unwrap();
        let source = folder.join(format!("data/input-{i}.fasta"));
        std::fs::write(&source, b">a\nACGT\n").unwrap();
        records::reference_input(
            &results.join("out.txt"),
            &source,
            &basecraft_io::hash_file(&source).unwrap(),
            None,
            &[],
            "index test",
            Some(&project),
        )
        .unwrap();
    }
    let target = folder.join("data/unrelated.fasta");
    std::fs::write(&target, b">b\nAAAA\n").unwrap();
    let mut session = basecraft_ops::Session::new(&folder).in_project(Some(project));
    let doc = session.open(&target).unwrap();
    session
        .apply(doc, basecraft_protocol::Op::seq_replace(0, 1, "T"))
        .unwrap();
    basecraft_io::counters::reset();
    session.save(doc, None).unwrap();
    let counts = basecraft_io::counters::snapshot();
    assert!(counts.store_inspections.is_empty());
    let bytes: u64 = counts.reads.values().map(|r| r.bytes).sum();
    assert!(bytes < 100_000, "{bytes} bytes read");
}
