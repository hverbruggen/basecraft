//! Facts a standard file can't hold live in the document's companion, which
//! exists only when there is a fact to keep.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{Session, files};
use basecraft_protocol::Op;
use basecraft_protocol::facts::{Alphabet, FactSource, StatedAlphabet};

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
    assert!(!files::companion_path(None, &path).exists());
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
                alphabet: StatedAlphabet::Rna,
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
                alphabet: StatedAlphabet::Protein,
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
    let companion = files::companion_path(None, &path);
    std::fs::create_dir_all(companion.parent().unwrap()).unwrap();
    std::fs::write(companion, bytes).unwrap();
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
                alphabet: StatedAlphabet::Protein,
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert!(files::companion_path(None, &path).exists());
    session.undo(doc).unwrap();
    session.save(doc, None).unwrap();
    assert!(!files::companion_path(None, &path).exists());
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
                alphabet: StatedAlphabet::Dna,
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
fn files_report_undetermined_and_mixed_sequences() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    // One E in 1,000 letters: basecraft can't tell.
    let one_e = format!("{}E", "ACGTACGTAC".repeat(100).split_off(1));
    let (session, doc) = open(dir.path(), "rare-e.fasta", &format!(">s\n{one_e}\n"));
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Undetermined);
    let mixed = ">dna\nACGTACGTAC\n>protein\nMKLVEFIQPA\n";
    let (session, doc) = open(dir.path(), "mixed.afa", mixed);
    let alphabet = session.alphabet(doc).unwrap();
    assert_eq!(
        (alphabet.value, alphabet.source),
        (Alphabet::Mixed, FactSource::Guessed)
    );
    let rows: Vec<_> = session
        .sequence_alphabets(doc)
        .unwrap()
        .into_iter()
        .map(|s| s.alphabet)
        .collect();
    assert_eq!(rows, [Alphabet::Dna, Alphabet::Protein]);
}

#[test]
fn the_guess_follows_edits_and_plugins_see_the_same_value() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let protein = (0..20)
        .map(|i| format!(">p{i}\nMKLVEFIQPA\n"))
        .collect::<Vec<_>>()
        .concat();
    let (mut session, doc) = open(
        dir.path(),
        "rows.afa",
        &format!("{protein}>odd\nACGTACGTAC\n"),
    );
    let refusal = session.alignment(doc).unwrap().alphabet_refusal().unwrap();
    assert!(refusal.contains("1 DNA: odd; 20 protein"), "{refusal}");
    session
        .apply(
            doc,
            Op::AlnRemoveRows {
                rows: vec![(20, 21)],
            },
        )
        .unwrap();
    let alignment = session.alignment(doc).unwrap();
    assert_eq!(alignment.alphabet().value, Alphabet::Protein);
    assert!(alignment.is_protein());
    assert_eq!(alignment.alphabet_refusal(), None);
    session.undo(doc).unwrap();
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Mixed);
    // Typing protein over the DNA row settles it too.
    session
        .apply(
            doc,
            Op::AlnReplace {
                row: 20,
                start: 0,
                end: 10,
                residues: "MKLVEFIQPA".into(),
            },
        )
        .unwrap();
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Protein);
}

#[test]
fn undetermined_sequences_are_named_until_an_alphabet_is_stated() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let dna = "ACGTACGTAC".repeat(100);
    let text = format!(">clear\n{dna}\n>unclear\n{}E\n", &dna[1..]);
    let (mut session, doc) = open(dir.path(), "rows.afa", &text);
    let refusal = session.alignment(doc).unwrap().alphabet_refusal().unwrap();
    assert!(
        refusal.contains("can't tell") && refusal.contains(": unclear."),
        "{refusal}"
    );
    let dna_stated = Op::SetAlphabet {
        alphabet: StatedAlphabet::Dna,
    };
    session.apply(doc, dna_stated).unwrap();
    assert_eq!(session.alignment(doc).unwrap().alphabet_refusal(), None);
}

#[test]
fn genbank_files_state_their_molecule_and_every_record_counts() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let record = |name: &str, unit: &str, molecule: &str, residues: &str| {
        format!(
            "LOCUS       {name:<16} {len:>11} {unit} {molecule:<7} linear   SYN 10-OCT-2026\nORIGIN\n        1 {residues}\n//\n",
            len = residues.len()
        )
    };
    // GenBank writes mRNA with T; the molecule type says RNA.
    let (session, doc) = open(dir.path(), "m.gb", &record("m", "bp", "mRNA", "acgtacgtac"));
    let alphabet = session.alphabet(doc).unwrap();
    assert_eq!(
        (alphabet.value, alphabet.source),
        (Alphabet::Rna, FactSource::Stated)
    );
    let both = record("d", "bp", "DNA", "acgtacgtac") + &record("p", "aa", "", "mklvefiqpa");
    let (session, doc) = open(dir.path(), "two.gb", &both);
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Mixed);
    let records: Vec<_> = session
        .sequence_alphabets(doc)
        .unwrap()
        .into_iter()
        .map(|s| (s.name, s.alphabet))
        .collect();
    assert_eq!(
        records,
        [("d".into(), Alphabet::Dna), ("p".into(), Alphabet::Protein)]
    );
    // A GenPept record first: "aa" states protein even when its residues
    // hold no protein-only letters, and the file's own default says nothing.
    let peptide = record("p", "aa", "", "mkvasghmkvasghmkvas") + &record("d", "bp", "DNA", "acgt");
    let (genpept, first) = open(dir.path(), "first.gb", &peptide);
    let records: Vec<_> = genpept
        .sequence_alphabets(first)
        .unwrap()
        .into_iter()
        .map(|s| s.alphabet)
        .collect();
    assert_eq!(records, [Alphabet::Protein, Alphabet::Dna]);
    let alphabet = genpept.alphabet(first).unwrap();
    assert_eq!(
        (alphabet.value, alphabet.source),
        (Alphabet::Mixed, FactSource::Stated)
    );
    // A plain open and save writes no side file for what the file says.
    let mut session = session;
    session.apply(doc, Op::seq_replace(0, 1, "G")).unwrap();
    session.save(doc, None).unwrap();
    assert!(!files::companion_path(None, &dir.path().join("two.gb")).exists());
}

/// 1,000 nucleotides, then protein-only letters the guess never reaches.
fn dna_then_protein() -> String {
    format!("{}{}", "ACGT".repeat(250), "MELQ".repeat(250))
}

#[test]
fn the_guess_counts_only_the_first_thousand_letters_of_a_sequence() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let text = format!(">s\n{}\n", dna_then_protein());
    let (mut session, doc) = open(dir.path(), "long.fasta", &text);
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Dna);
    // Beyond the counted prefix an edit changes nothing, even one that
    // would turn the whole sequence into protein.
    session
        .apply(doc, Op::seq_replace(1000, 2000, "E".repeat(800)))
        .unwrap();
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Dna);
    // Inside it, the prefix is counted again: 20 E among the first 1,000.
    session
        .apply(doc, Op::seq_replace(0, 20, "E".repeat(20)))
        .unwrap();
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Protein);
    // Deleting letters that were never counted can't take the counts below zero.
    session.apply(doc, Op::seq_replace(0, 1700, "")).unwrap();
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Protein);
    session.undo(doc).unwrap();
    session.undo(doc).unwrap();
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Dna);
}

#[test]
fn every_row_of_a_long_alignment_is_guessed_from_its_first_thousand_letters() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let row = dna_then_protein();
    let text = format!(">a\n{row}\n>b\n{row}\n>short\nAC\n");
    let (mut session, doc) = open(dir.path(), "long.afa", &text);
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Dna);
    let replace = |row, start, end, residues: &str| Op::AlnReplace {
        row,
        start,
        end,
        residues: residues.into(),
    };
    session
        .apply(doc, replace(0, 1500, 1600, &"E".repeat(100)))
        .unwrap();
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Dna);
    session
        .apply(doc, replace(0, 0, 50, &"E".repeat(50)))
        .unwrap();
    let rows: Vec<_> = session
        .sequence_alphabets(doc)
        .unwrap()
        .into_iter()
        .map(|s| s.alphabet)
        .collect();
    assert_eq!(rows, [Alphabet::Protein, Alphabet::Dna, Alphabet::Dna]);
    assert_eq!(session.alphabet(doc).unwrap().value, Alphabet::Mixed);
}
