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
    let edit = Op::SeqReplace {
        start: 0,
        end: 1,
        residues: "T".into(),
    };
    session.apply(doc, edit).unwrap();
    session.save(doc, None).unwrap();
    let mut reopened = Session::new(dir.path());
    let doc = reopened.open(&path).unwrap();
    assert_eq!(reopened.alphabet(doc).unwrap().source, FactSource::User);
    assert_eq!(reopened.opening_warning(doc).unwrap(), None);
}
