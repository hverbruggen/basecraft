//! Saving keeps every record and annotation the document does not show.
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use std::path::Path;

use basecraft_ops::{Session, files::Kind};
use basecraft_protocol::{Op, ops::DocumentCounts};

const LOCUS: &str =
    include_str!("../../../testdata/fixtures/compact/sequences-annotations/locus.gb");

fn replace() -> Op {
    Op::SeqReplace {
        start: 0,
        end: 1,
        residues: "GG".into(),
    }
}

/// The bytes after the first `//` line: the records the document does not show.
fn after_first(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .windows(3)
        .position(|w| w == b"\n//")
        .expect("first record end");
    let next = end + 1 + bytes[end + 1..].iter().position(|&b| b == b'\n').unwrap() + 1;
    &bytes[next..]
}

fn records(session: &Session, doc: basecraft_ops::DocId) -> Option<u64> {
    match session.state(doc).unwrap().counts {
        Some(DocumentCounts::Sequence { records, .. }) => records,
        other => panic!("not a sequence: {other:?}"),
    }
}

#[test]
fn genbank_keeps_unseen_records_byte_for_byte() {
    let Some(root) = isolated_profile::enter("genbank_keeps_unseen_records_byte_for_byte") else {
        return;
    };
    let crlf = LOCUS.replace('\n', "\r\n");
    let open_ended = LOCUS.trim_end().trim_end_matches("//");
    let cases = [
        ("two.gb", format!("{LOCUS}{LOCUS}"), 2),
        ("three.gb", format!("{LOCUS}\n{crlf}{open_ended}\n\n\n"), 3),
    ];
    for (name, original, count) in cases {
        let path = root.join(name);
        std::fs::write(&path, &original).unwrap();
        let mut session = Session::recoverable(&root).unwrap();
        let doc = session.open(&path).unwrap();
        assert_eq!(records(&session, doc), Some(count));
        session.apply(doc, replace()).unwrap();
        let copy = root.join(format!("copy-{name}"));
        session.save(doc, None).unwrap();
        session.save(doc, Some(&copy)).unwrap();
        for saved in [&path, &copy] {
            let bytes = std::fs::read(saved).unwrap();
            assert_eq!(
                after_first(&bytes),
                after_first(original.as_bytes()),
                "{name}"
            );
            let parsed = basecraft_io::genbank::parse(&bytes).unwrap();
            assert_eq!(parsed.len() as u64, count);
            assert!(parsed[0].sequence.starts_with(b"GG"));
        }
    }
}

#[test]
fn fasta_companion_keeps_other_ids_header_and_regions() {
    let Some(root) = isolated_profile::enter("fasta_companion_keeps_other_ids_header_and_regions")
    else {
        return;
    };
    // The second record's 70-column lines must not be rewrapped.
    let second = format!(
        ">second kept as is\n{}\n{}\n",
        "T".repeat(70),
        "A".repeat(5)
    );
    let original = format!(">first\nACGTACGT\n{second}");
    let gff = "##gff-version 3\n# made by hand\n##sequence-region first 1 8\n##sequence-region second 1 75\n\
               first\t.\tgene\t2\t4\t.\t+\t.\tID=one\nsecond\tsrc\tgene\t10\t20\t.\t-\t.\tID=two;Note=kept%3B\n";
    let path = root.join("records.fa");
    std::fs::write(&path, &original).unwrap();
    std::fs::write(basecraft_ops::files::gff_sidecar(&path), gff).unwrap();
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&path).unwrap();
    assert_eq!(records(&session, doc), Some(2));
    session.apply(doc, replace()).unwrap();
    let copy = root.join("copy.fa");
    session.save(doc, None).unwrap();
    session.save(doc, Some(&copy)).unwrap();
    for saved in [&path, &copy] {
        let main = std::fs::read_to_string(saved).unwrap();
        assert_eq!(main, format!(">first\nGGCGTACGT\n{second}"));
        let companion = std::fs::read_to_string(basecraft_ops::files::gff_sidecar(saved)).unwrap();
        assert_eq!(
            companion,
            "##gff-version 3\n# made by hand\n##sequence-region first 1 9\n##sequence-region second 1 75\n\
             first\t.\tgene\t3\t5\t.\t+\t.\tID=one\nsecond\tsrc\tgene\t10\t20\t.\t-\t.\tID=two;Note=kept%3B\n"
        );
    }
}

#[test]
fn opening_without_edits_changes_nothing() {
    let Some(root) = isolated_profile::enter("opening_without_edits_changes_nothing") else {
        return;
    };
    let gb = root.join("two.gb");
    std::fs::write(&gb, format!("{LOCUS}{LOCUS}")).unwrap();
    let fa = root.join("two.fa");
    std::fs::write(&fa, ">a\nACGT\n>b\nTTTT\n").unwrap();
    std::fs::write(
        basecraft_ops::files::gff_sidecar(&fa),
        "##gff-version 3\nb\t.\tgene\t1\t2\t.\t+\t.\tID=x\n",
    )
    .unwrap();
    let before = listing(&root);
    let mut session = Session::recoverable(&root).unwrap();
    for path in [&gb, &fa] {
        session.open(path).unwrap();
    }
    drop(session);
    assert_eq!(listing(&root), before);
}

fn listing(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push((path.clone(), std::fs::read(path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn annotated_alignment_saves_and_leaves_its_companion() {
    let Some(root) = isolated_profile::enter("annotated_alignment_saves_and_leaves_its_companion")
    else {
        return;
    };
    let path = root.join("rows.afa");
    std::fs::write(&path, ">first\nACGT\n>second\nTTTT\n").unwrap();
    let gff = "##gff-version 3\nsecond\t.\tgene\t1\t4\t.\t+\t.\tID=two\n";
    let companion = basecraft_ops::files::gff_sidecar(&path);
    std::fs::write(&companion, gff).unwrap();
    let mut session = Session::recoverable(&root).unwrap();
    let (doc, kind) = session.open_classified(&path).unwrap();
    assert_eq!(kind, Kind::Alignment);
    session
        .apply(
            doc,
            Op::AlnReplace {
                row: 0,
                start: 0,
                end: 1,
                residues: "G".into(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        ">first\nGCGT\n>second\nTTTT\n"
    );
    assert_eq!(std::fs::read_to_string(companion).unwrap(), gff);
}

#[test]
fn recovery_restores_every_record() {
    let Some(root) = isolated_profile::enter("recovery_restores_every_record") else {
        return;
    };
    let path = root.join("two.gb");
    let original = format!("{LOCUS}{LOCUS}");
    std::fs::write(&path, &original).unwrap();
    let mut session = Session::recoverable(&root).unwrap();
    let doc = session.open(&path).unwrap();
    session.apply(doc, replace()).unwrap();
    session.flush().unwrap();
    let journal = session.recovery_root().to_path_buf();
    std::mem::forget(session);
    let recovered = basecraft_ops::records::recover(&journal, &root.join("recovered")).unwrap();
    let bytes = std::fs::read(&recovered[0]).unwrap();
    assert_eq!(after_first(&bytes), after_first(original.as_bytes()));
    assert_eq!(basecraft_io::genbank::parse(&bytes).unwrap().len(), 2);
}
