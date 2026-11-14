//! Every FASTA and GenBank file in testdata opens with the alphabet the D12
//! rule gives: protein for `sequences-annotations/protein.fasta`, DNA for the
//! rest. Run with
//! `cargo test -p basecraft-ops --test integration alphabet_testdata:: -- --ignored`.
use std::path::{Path, PathBuf};

use basecraft_ops::Session;
use basecraft_protocol::facts::Alphabet;

/// Invalid on purpose: they don't open.
const DO_NOT_OPEN: [&str; 3] = [
    "recovery-errors/invalid/malformed.gb",
    "recovery-errors/invalid/empty.fasta",
    "recovery-errors/invalid/missing-header.fasta",
];

fn sequence_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for path in std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if path.is_dir() {
            sequence_files(&path, out);
        } else if ["fasta", "fa", "afa", "fas", "fna", "faa", "gb", "gbk"].contains(&ext) {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "checks the testdata tree; run by hand when the guesser changes"]
fn testdata_alphabets_follow_d12() {
    let mut files = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    sequence_files(&root, &mut files);
    let mut skipped = Vec::new();
    for file in files {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join(file.file_name().unwrap());
        std::fs::copy(&file, &copy).unwrap();
        let mut session = Session::new(dir.path());
        let Ok(doc) = session.open(&copy) else {
            skipped.push(file);
            continue;
        };
        let expected = if file.ends_with("sequences-annotations/protein.fasta") {
            Alphabet::Protein
        } else {
            Alphabet::Dna
        };
        let found = session.alphabet(doc).unwrap().value;
        assert_eq!(found, expected, "{}", file.display());
    }
    assert_eq!(skipped.len(), DO_NOT_OPEN.len(), "{skipped:?}");
    assert!(
        DO_NOT_OPEN
            .iter()
            .all(|name| skipped.iter().any(|f| f.ends_with(name)))
    );
}
