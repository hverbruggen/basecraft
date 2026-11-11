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
    let mut session = Session::recoverable_with_paths(&root, &paths).unwrap();
    let (doc, _) = session.open_classified(&file).unwrap();
    record(
        name,
        "open",
        &file,
        (open_reads, open_reads * original_size, 0, 0),
    );
    for i in 0..3 {
        counters::reset();
        session.apply(doc, edit(i)).unwrap();
        let expected = (0, 0, 1, 0);
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
        1,
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

#[test]
fn companions_are_read_once() {
    let Some(root) = child_profile("companions_are_read_once") else {
        return;
    };
    let fasta = root.join("sequence.fa");
    std::fs::write(&fasta, b">sequence\nACGT\n").unwrap();
    std::fs::write(
        fasta.with_extension("gff3"),
        b"##gff-version 3\nsequence\t.\tgene\t1\t3\t.\t+\t.\tID=g\n",
    )
    .unwrap();
    let metadata = basecraft_ops::metadata::create(&root).unwrap();
    let alignment = root.join("source.afa");
    std::fs::write(&alignment, b">a\nACGT\n>b\nACGA\n").unwrap();
    let submission = root.join("review.submission.json");
    basecraft_ops::submission::create(&mut Session::unlogged(&root), &submission, &alignment)
        .unwrap();
    let tree = root.join("tree.nwk");
    std::fs::write(&tree, b"(a:1,b:2);\n").unwrap();
    let mut writer = Session::unlogged(&root);
    let doc = writer.open(&tree).unwrap();
    writer.save(doc, None).unwrap();
    let spart = root.join("species.spart");
    std::fs::write(
        &spart,
        include_bytes!("../../../testdata/fixtures/delimitation/manual.spart"),
    )
    .unwrap();
    let doc = writer.open(&spart).unwrap();
    writer.save(doc, None).unwrap();
    for path in [&fasta, &metadata, &submission, &tree, &spart] {
        counters::reset();
        let mut session = Session::unlogged(&root);
        let doc = session.open(path).unwrap();
        assert!(!session.document_record(doc).unwrap().unsaved);
        for (file, reads) in counters::snapshot().reads {
            assert_eq!(
                reads.full_reads,
                1,
                "{} while opening {}",
                file.display(),
                path.display()
            );
            assert_eq!(reads.bytes, file.metadata().unwrap().len());
        }
    }
}

#[test]
fn large_file_edits_and_recovery_require_original() {
    use std::io::Write;
    let Some(root) = child_profile("large_file_edits_and_recovery_require_original") else {
        return;
    };
    let file = root.join("large.fa");
    let mut output = std::fs::File::create(&file).unwrap();
    output.write_all(b">large\n").unwrap();
    let chunk = vec![b'A'; 1 << 20];
    for _ in 0..101 {
        output.write_all(&chunk).unwrap();
    }
    drop(output);
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&file).unwrap();
    let recovery = session.recovery_root().to_path_buf();
    counters::reset();
    session
        .apply(
            doc,
            Op::SeqReplace {
                start: 0,
                end: 1,
                residues: "C".into(),
            },
        )
        .unwrap();
    assert!(counters::snapshot().reads.is_empty());
    assert_eq!(counters::snapshot().sync_all, 0);
    assert_eq!(counters::snapshot().sync_data, 1);
    drop(session);
    let recovered = basecraft_ops::replay::recover(&recovery, &root.join("recovered")).unwrap();
    let mut reopened = Session::unlogged(&root);
    let doc = reopened.open(&recovered[0]).unwrap();
    assert_eq!(reopened.doc(doc).unwrap().read(0, 4), b"CAAA");
    // Saving over the original keeps its inode as a reference, without copying
    // the file; later undo and new edits still recover through the same journal.
    let mut saving = Session::recoverable(&root).unwrap();
    let current = saving.open(&file).unwrap();
    saving
        .apply(
            current,
            Op::SeqReplace {
                start: 0,
                end: 1,
                residues: "G".into(),
            },
        )
        .unwrap();
    saving.save(current, None).unwrap();
    saving.undo(current).unwrap();
    let saved_recovery = saving.recovery_root().to_path_buf();
    drop(saving);
    let saved = basecraft_ops::replay::recover(&saved_recovery, &root.join("after-save")).unwrap();
    let mut verify = Session::unlogged(&root);
    let current = verify.open(&saved[0]).unwrap();
    assert_eq!(verify.doc(current).unwrap().read(0, 4), b"AAAA");
    std::fs::write(&file, b">large\nTTTT\n").unwrap();
    let error = basecraft_ops::replay::recover(&recovery, &root.join("changed")).unwrap_err();
    assert!(error.to_string().contains("unchanged original"));
    assert!(!root.join("changed").exists());
    std::fs::remove_file(&file).unwrap();
    assert!(basecraft_ops::replay::recover(&recovery, &root.join("missing")).is_err());
}
