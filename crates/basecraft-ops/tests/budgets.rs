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
fn scenario(name: &str, fixture: &str, open_reads: u64, edit: impl Fn(u32) -> Op) {
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
    // Saving reads the original once; history keeps fingerprints, not copies.
    // Staged data and history files sync on worker threads, outside this count.
    record(name, "save", &file, (1, original_size, 1, 0));
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
    scenario("fasta", "alignment-editing/ambiguous-gappy.afa", 1, |i| {
        Op::AlnReplace {
            row: 0,
            start: u64::from(i),
            end: u64::from(i) + 1,
            residues: "C".into(),
        }
    });
}

#[test]
fn genbank() {
    scenario("genbank", "sequences-annotations/locus.gb", 1, |i| {
        Op::seq_replace(u64::from(i), u64::from(i) + 1, "C")
    });
}

#[test]
fn newick() {
    scenario("newick", "tree-inference/unrooted.nwk", 1, |i| {
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
        let expected: std::collections::BTreeSet<_> = if path == &metadata {
            basecraft_ops::metadata::files(path)
        } else if path == &fasta {
            vec![path.clone(), path.with_extension("gff3")]
        } else if path == &tree || path == &spart {
            vec![path.clone(), basecraft_ops::files::tree_sidecar(path)]
        } else {
            vec![path.clone()]
        }
        .into_iter()
        .map(|p| p.canonicalize().unwrap())
        .collect();
        assert_eq!(
            counters::snapshot()
                .reads
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
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
fn large_first_edit_records_the_original_without_reading() {
    use std::io::Write;
    let Some(root) = child_profile("large_first_edit_records_the_original_without_reading") else {
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
            Op::seq_replace(0, 1, "C"),
        )
        .unwrap();
    assert!(counters::snapshot().reads.is_empty());
    assert_eq!(counters::snapshot().sync_all, 0);
    assert_eq!(counters::snapshot().sync_data, 1);
    drop(session);
    let recovered =
        basecraft_ops::records::recover(&recovery, &root.join("recovered"), None).unwrap();
    let mut reopened = Session::unlogged(&root);
    let doc = reopened.open(&recovered[0]).unwrap();
    assert_eq!(reopened.doc(doc).unwrap().read(0, 4), b"CAAA");
    // No private copy is kept: without the original nothing is recovered.
    std::fs::remove_file(&file).unwrap();
    assert!(
        basecraft_ops::records::recover(&recovery, &root.join("missing"), None)
            .map_or(true, |outputs| outputs.is_empty())
    );
}
