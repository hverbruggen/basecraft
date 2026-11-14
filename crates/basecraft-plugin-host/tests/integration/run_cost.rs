//! What one plugin run on a large alignment costs: the files it leaves, the
//! bytes this process reads, and the time it takes. The numbers are printed
//! (and appended to the file `RUN_COST_OUT` names, since the test runs in a
//! child process); the test checks only that the run leaves its records.
use crate::common;
use basecraft_ops::Session;
use serde_json::json;
use std::{collections::BTreeSet, fmt::Write as _, path::Path, time::Instant};

fn files(root: &Path, out: &mut BTreeSet<std::path::PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            files(&entry.path(), out);
        } else {
            out.insert(entry.path());
        }
    }
}

/// Bytes this process has read through system calls so far (Linux only).
fn bytes_read() -> u64 {
    std::fs::read_to_string("/proc/self/io")
        .ok()
        .and_then(|io| {
            io.lines()
                .find_map(|line| line.strip_prefix("rchar: ")?.trim().parse().ok())
        })
        .unwrap_or(0)
}

fn measured(label: &str, project: &Path, input: u64, run: impl FnOnce()) -> usize {
    let mut before = BTreeSet::new();
    files(project, &mut before);
    let (read, start) = (bytes_read(), Instant::now());
    run();
    let (elapsed, read) = (start.elapsed(), bytes_read() - read);
    let mut after = BTreeSet::new();
    files(project, &mut after);
    let new: Vec<_> = after.difference(&before).collect();
    let mut names = String::new();
    for path in &new {
        let size = std::fs::metadata(path).unwrap().len();
        let name = path.strip_prefix(project).unwrap().display();
        writeln!(names, "  {size:>10}  {name}").unwrap();
    }
    let tenths = read * 10 / input;
    let line = format!(
        "{label}: {} ms, {} new files, {read} bytes read ({}.{} x input)\n{names}",
        elapsed.as_millis(),
        new.len(),
        tenths / 10,
        tenths % 10
    );
    println!("{line}");
    if let Some(out) = std::env::var_os("RUN_COST_OUT") {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out)
            .unwrap();
        file.write_all(line.as_bytes()).unwrap();
    }
    new.len()
}

#[test]
fn a_plugin_run_on_a_large_alignment() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let project = project.canonicalize().unwrap();
    let mut text = String::new();
    let row = "ACGT".repeat(160_000);
    for name in 0..100 {
        writeln!(text, ">s{name}").unwrap();
        for line in row.as_bytes().chunks(80) {
            text.push_str(std::str::from_utf8(line).unwrap());
            text.push('\n');
        }
    }
    let source = project.join("big.fasta");
    std::fs::write(&source, &text).unwrap();
    let input = text.len() as u64;
    drop(text);
    let registry = common::copy_aligner(dir.path());

    let from_file = measured("file input", &project, input, || {
        let outcome = common::run(
            &registry,
            &project,
            json!({"operation":"basecraft.mafft/align@1","inputs":{"alignment":common::file("big.fasta")},
                "parameters":{},"destinations":{"artifacts/alignment.afa":project.join("from-file.fasta")}}),
        );
        common::assert_completed(&outcome);
    });
    let mut session = Session::recoverable(&project).unwrap();
    let output = project.join("from-document.fasta");
    let from_document = measured("open document input", &project, input, || {
        let doc = session.open(&source).unwrap();
        let outcome = common::run_in(&registry, &mut session, |s| {
            json!({"operation":"basecraft.mafft/align@1","inputs":{"alignment":common::document(s, doc)},
                "parameters":{},"destinations":{"artifacts/alignment.afa":output}})
        });
        common::assert_completed(&outcome);
    });
    assert!(from_file > 1 && from_document > 1);
}
