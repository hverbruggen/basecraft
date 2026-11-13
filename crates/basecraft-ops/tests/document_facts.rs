//! Facts a standard file can't hold live in the document's companion, which
//! exists only when there is a fact to keep.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{Session, files};
use basecraft_protocol::Op;
use basecraft_protocol::facts::{Alphabet, FactSource};

#[test]
fn a_plain_open_and_save_writes_no_companion() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pair.afa");
    std::fs::write(&path, ">a\nACGT\n>b\nACGA\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    session
        .apply(
            doc,
            Op::AlnRenameRow {
                row: 1,
                name: "c".into(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert!(!files::tree_sidecar(&path).exists());
}

#[test]
fn a_stated_alphabet_survives_save_and_reopen_for_sequences_too() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.fasta");
    std::fs::write(&path, ">x\nACGUACGU\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    assert_eq!(session.alphabet(doc).unwrap().source, FactSource::Guessed);
    session
        .apply(
            doc,
            Op::SetAlphabet {
                alphabet: Alphabet::Rna,
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), ">x\nACGUACGU\n");
    let mut reopened = Session::new(dir.path());
    let doc = reopened.open(&path).unwrap();
    let alphabet = reopened.alphabet(doc).unwrap();
    assert_eq!(
        (alphabet.value, alphabet.source),
        (Alphabet::Rna, FactSource::User)
    );
}

#[test]
fn an_edit_made_elsewhere_sets_the_companion_aside_with_a_warning() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pair.afa");
    std::fs::write(&path, ">a\nACGT\n>b\nACGA\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    session
        .apply(
            doc,
            Op::SetAlphabet {
                alphabet: Alphabet::Protein,
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    // Another program changes the alignment; the stated alphabet no longer applies.
    std::fs::write(&path, ">a\nACGT\n>b\nACGG\n").unwrap();
    let mut reopened = Session::new(dir.path());
    let doc = reopened.open(&path).unwrap();
    assert_eq!(reopened.alphabet(doc).unwrap().source, FactSource::Guessed);
    let warning = reopened.opening_warning(doc).unwrap().unwrap();
    assert!(warning.contains("changed outside basecraft"), "{warning}");
}

#[test]
fn a_companion_for_another_tree_is_reported() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.nwk");
    std::fs::write(&path, "((a,b),c);\n").unwrap();
    let other = basecraft_docs::TreeDoc::from_newick("t", "((a,c),b);").unwrap();
    let hash = basecraft_io::hash_file(&path).unwrap();
    let mut bytes = Vec::new();
    files::tree_companion(&other, &hash)
        .write(&mut bytes)
        .unwrap();
    std::fs::write(files::tree_sidecar(&path), bytes).unwrap();
    let warning = files::load_tree(&path).unwrap().metadata_warning.unwrap();
    assert!(warning.contains("different tree"), "{warning}");
}

#[test]
fn an_undone_alphabet_does_not_come_back_on_reopen() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pair.afa");
    std::fs::write(&path, ">a\nACGT\n>b\nACGA\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    session
        .apply(
            doc,
            Op::SetAlphabet {
                alphabet: Alphabet::Protein,
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert!(files::tree_sidecar(&path).exists());
    session.undo(doc).unwrap();
    session.save(doc, None).unwrap();
    assert!(!files::tree_sidecar(&path).exists());
    let mut reopened = Session::new(dir.path());
    let doc = reopened.open(&path).unwrap();
    assert_eq!(reopened.alphabet(doc).unwrap().source, FactSource::Guessed);
}

#[test]
fn a_sequence_with_a_stated_alphabet_saves_again_after_an_edit() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.fasta");
    std::fs::write(&path, ">x\nACGTACGT\n").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    session
        .apply(
            doc,
            Op::SetAlphabet {
                alphabet: Alphabet::Dna,
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    let edit = Op::seq_replace(0, 1, "T");
    session.apply(doc, edit).unwrap();
    session.save(doc, None).unwrap();
    let mut reopened = Session::new(dir.path());
    let doc = reopened.open(&path).unwrap();
    assert_eq!(reopened.alphabet(doc).unwrap().source, FactSource::User);
    assert_eq!(reopened.opening_warning(doc).unwrap(), None);
}

/// Open `text` as `name` and return the session and document.
fn open(dir: &std::path::Path, name: &str, text: &str) -> (Session, basecraft_ops::DocId) {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let mut session = Session::new(dir);
    let doc = session.open(&path).unwrap();
    (session, doc)
}

#[test]
fn the_guess_follows_d12_per_sequence() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let dna = "ACGTACGTAC".repeat(100);
    let one_e = format!("{}E", &dna[1..]);
    for (name, text, expected) in [
        (
            "x.afa",
            format!(">a\n{dna}\n>b\nACGTX{}\n", &dna[5..]),
            Alphabet::Dna,
        ),
        (
            "iupac.afa",
            ">a\nACGRYKMSWBDHVN\n>b\nRYKMSWACGTNNNN\n".into(),
            Alphabet::Dna,
        ),
        (
            "rna.afa",
            ">a\nACGUACGU\n>b\nUUGCA-GU\n".into(),
            Alphabet::Rna,
        ),
        (
            "rare-e.fasta",
            format!(">s\n{one_e}\n"),
            Alphabet::Undetermined,
        ),
        (
            "mixed.afa",
            format!(">dna\n{}\n>protein\nMKLVEFIQPA\n", &dna[..10]),
            Alphabet::Mixed,
        ),
    ] {
        let (session, doc) = open(dir.path(), name, &text);
        let alphabet = session.alphabet(doc).unwrap();
        assert_eq!((name, alphabet.value), (name, expected));
        assert_eq!(alphabet.source, FactSource::Guessed);
    }
    let (session, doc) = open(
        dir.path(),
        "mixed2.afa",
        ">dna\nACGTACGTAC\n>protein\nMKLVEFIQPA\n",
    );
    let rows: Vec<_> = session
        .sequence_alphabets(doc)
        .unwrap()
        .into_iter()
        .map(|s| (s.name, s.alphabet))
        .collect();
    assert_eq!(
        rows,
        [
            ("dna".into(), Alphabet::Dna),
            ("protein".into(), Alphabet::Protein)
        ]
    );
}

#[test]
fn undetermined_or_mixed_sequences_are_named_until_an_alphabet_is_stated() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let dna = "ACGTACGTAC".repeat(100);
    let text = format!(">clear\n{dna}\n>unclear\n{}E\n", &dna[1..]);
    let (mut session, doc) = open(dir.path(), "rows.afa", &text);
    let refusal = session.alignment(doc).unwrap().alphabet_refusal().unwrap();
    assert!(
        refusal.contains("can't tell") && refusal.contains("unclear (Undetermined)"),
        "{refusal}"
    );
    assert!(!refusal.contains("clear (Dna)"), "{refusal}");
    session
        .apply(
            doc,
            Op::SetAlphabet {
                alphabet: Alphabet::Dna,
            },
        )
        .unwrap();
    assert_eq!(session.alignment(doc).unwrap().alphabet_refusal(), None);
    assert!(
        session
            .apply(
                doc,
                Op::SetAlphabet {
                    alphabet: Alphabet::Mixed
                }
            )
            .is_err()
    );
}
