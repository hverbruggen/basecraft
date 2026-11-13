//! The answer to "what is this sequence?" is written at once and asked
//! once (D22).
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{DocId, Session};
use basecraft_protocol::Op;
use basecraft_protocol::facts::{Alphabet, FactSource, StatedAlphabet};
use std::path::Path;

/// One E in 1,000 letters: basecraft can't tell what this is.
fn unclear() -> String {
    format!(">s\n{}E\n", "ACGTACGTAC".repeat(100).split_off(1))
}

fn open(root: &Path) -> (Session, DocId) {
    let mut session = Session::new(root);
    let doc = session.open(&root.join("s.fasta")).unwrap();
    (session, doc)
}

fn answer(alphabet: StatedAlphabet) -> Op {
    Op::SetAlphabet { alphabet }
}

#[test]
fn the_answer_is_kept_without_a_save_and_is_one_undo_step() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("s.fasta");
    std::fs::write(&file, unclear()).unwrap();
    let side = dir.path().join("s.fasta.basecraft.json");
    let (mut session, doc) = open(dir.path());
    assert!(session.doc(doc).unwrap().alphabet_refusal().is_some());
    session.apply(doc, answer(StatedAlphabet::Dna)).unwrap();
    assert!(side.exists(), "the answer is in the side file at once");
    assert!(!session.is_unsaved(doc), "nothing is left to save");
    // One undo step, written at once too.
    session.undo(doc).unwrap();
    assert!(!side.exists());
    assert!(!session.is_unsaved(doc));
    session.redo(doc).unwrap();
    assert!(side.exists());
    let logged = basecraft_ops::log::read_all(dir.path()).unwrap();
    assert!(
        logged
            .iter()
            .any(|e| matches!(e.op, Op::SetAlphabet { .. })),
        "the answer is journaled"
    );
    drop(session);

    // Closed without saving: the answer is still there, the file unchanged.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), unclear());
    let (mut session, doc) = open(dir.path());
    let alphabet = session.alphabet(doc).unwrap();
    assert_eq!(
        (alphabet.value, alphabet.source),
        (Alphabet::Dna, FactSource::User)
    );
    assert_eq!(session.doc(doc).unwrap().alphabet_refusal(), None);
    // A later edit and save keep the answer and accept the side file as written.
    session.apply(doc, Op::seq_replace(0, 1, "T")).unwrap();
    session.save(doc, None).unwrap();
    let (session, doc) = open(dir.path());
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Dna);
}

#[test]
fn an_answer_on_an_edited_document_leaves_the_edits_to_save() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("s.fasta"), unclear()).unwrap();
    let (mut session, doc) = open(dir.path());
    session.apply(doc, Op::seq_replace(0, 1, "T")).unwrap();
    session.apply(doc, answer(StatedAlphabet::Protein)).unwrap();
    assert!(session.is_unsaved(doc));
    drop(session);
    let (session, doc) = open(dir.path());
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Protein);
    assert_eq!(
        session.doc(doc).unwrap().read(0, 1),
        b"C",
        "the edit was not saved"
    );
}

#[test]
fn a_declined_answer_is_kept_and_analyses_still_refuse() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("s.fasta"), unclear()).unwrap();
    let (mut session, doc) = open(dir.path());
    session
        .apply(doc, answer(StatedAlphabet::Undetermined))
        .unwrap();
    drop(session);
    let (mut session, doc) = open(dir.path());
    let alphabet = session.alphabet(doc).unwrap();
    assert_eq!(
        (alphabet.value, alphabet.source),
        (Alphabet::Undetermined, FactSource::User)
    );
    let refusal = session.doc(doc).unwrap().alphabet_refusal().unwrap();
    assert!(refusal.contains("State the alphabet"), "{refusal}");
    // Stating it later through the alphabet setting lifts the refusal.
    session.apply(doc, answer(StatedAlphabet::Dna)).unwrap();
    assert_eq!(session.doc(doc).unwrap().alphabet_refusal(), None);
}

#[test]
fn a_group_mixing_an_answer_with_an_edit_still_needs_a_save() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("s.fasta"), unclear()).unwrap();
    let (mut session, doc) = open(dir.path());
    session.begin_group();
    session.apply(doc, Op::seq_replace(0, 1, "T")).unwrap();
    session.apply(doc, answer(StatedAlphabet::Dna)).unwrap();
    session.end_group();
    assert!(session.is_unsaved(doc));
    session.undo(doc).unwrap();
    assert!(!session.is_unsaved(doc), "undone back to the file on disk");
    session.redo(doc).unwrap();
    assert!(session.is_unsaved(doc), "the redone edit is not on disk");
}

#[test]
fn a_side_file_changed_on_disk_is_left_alone_and_the_answer_waits() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("s.fasta"), unclear()).unwrap();
    let side = dir.path().join("s.fasta.basecraft.json");
    let (mut session, doc) = open(dir.path());
    // Another program writes the side file after the session read the folder.
    std::fs::write(&side, b"written elsewhere").unwrap();
    let outcome = session.apply(doc, answer(StatedAlphabet::Dna)).unwrap();
    assert!(
        outcome
            .notes
            .iter()
            .any(|n| n.contains("kept until the next save")),
        "{:?}",
        outcome.notes
    );
    assert_eq!(std::fs::read(&side).unwrap(), b"written elsewhere");
    assert!(session.is_unsaved(doc));
}

#[test]
fn an_undo_that_cannot_write_the_side_file_says_so() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("s.fasta"), unclear()).unwrap();
    let side = dir.path().join("s.fasta.basecraft.json");
    let (mut session, doc) = open(dir.path());
    session.apply(doc, answer(StatedAlphabet::Dna)).unwrap();
    assert!(side.exists());
    std::fs::write(&side, b"changed elsewhere").unwrap();
    session.undo(doc).unwrap();
    let notice = session.recovery_warning(doc).unwrap_or_default();
    assert!(notice.contains("kept until the next save"), "{notice}");
    assert_eq!(std::fs::read(&side).unwrap(), b"changed elsewhere");
    assert!(session.is_unsaved(doc));
}
