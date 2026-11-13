//! Every FASTA and GenBank file in testdata, guessed by the D12 rule, set
//! against the guessers it replaced (alignments from the first 2000 columns
//! of 20 rows, sequences from all residues, protein when any letter is
//! outside the IUPAC nucleotide codes). Files whose protein-or-not answer
//! changed are listed in `CHANGED_BY_D12`. Run with
//! `cargo test -p basecraft-ops --test alphabet_testdata -- --ignored`.
use std::path::{Path, PathBuf};

use basecraft_ops::Session;
use basecraft_protocol::facts::Alphabet;

/// Files whose protein-or-not answer the D12 rule changed.
const CHANGED_BY_D12: [&str; 0] = [];

/// Invalid on purpose: they don't open.
const DO_NOT_OPEN: [&str; 3] = [
    "recovery-errors/invalid/malformed.gb",
    "recovery-errors/invalid/empty.fasta",
    "recovery-errors/invalid/missing-header.fasta",
];

fn old_guess(residues: &[u8]) -> bool {
    let nucleotide = |b: &u8| b"ACGTUNRYKMSWBDHV".contains(&b.to_ascii_uppercase());
    residues
        .iter()
        .any(|b| b.is_ascii_alphabetic() && !nucleotide(b))
}

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
fn testdata_alphabets_are_unchanged() {
    let mut files = Vec::new();
    sequence_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata"),
        &mut files,
    );
    let mut skipped = Vec::new();
    let mut changed = Vec::new();
    for file in files {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join(file.file_name().unwrap());
        std::fs::copy(&file, &copy).unwrap();
        let mut session = Session::new(dir.path());
        let Ok(doc) = session.open(&copy) else {
            skipped.push(file);
            continue;
        };
        let old = if let Ok(alignment) = session.alignment(doc) {
            let mut sample = Vec::new();
            for row in 0..alignment.row_count().min(20) {
                alignment.read_into(row, 0, alignment.width().min(2000), &mut sample);
            }
            old_guess(&sample)
        } else {
            old_guess(session.blast_records(doc).unwrap()[0].sequence.as_bytes())
        };
        let alphabet = session.alphabet(doc).unwrap().value;
        println!("checked {} {alphabet:?}", file.display());
        if old != (alphabet == Alphabet::Protein) {
            println!("changed {} {alphabet:?}", file.display());
            changed.push(file);
        }
    }
    assert_eq!(changed.len(), CHANGED_BY_D12.len(), "{changed:?}");
    assert!(
        CHANGED_BY_D12
            .iter()
            .all(|name| changed.iter().any(|f| f.ends_with(name)))
    );
    assert_eq!(skipped.len(), DO_NOT_OPEN.len(), "{skipped:?}");
    assert!(
        DO_NOT_OPEN
            .iter()
            .all(|name| skipped.iter().any(|f| f.ends_with(name)))
    );
}
