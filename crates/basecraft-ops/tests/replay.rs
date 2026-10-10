//! Record sessions, then replay the log and check every step.

use std::path::Path;

use basecraft_ops::{Session, replay};
use basecraft_protocol::{Feature, Location, Op, Qualifier, Strand};

const GB: &str = "LOCUS       TEST                     120 bp    DNA     circular SYN 10-OCT-2026
DEFINITION  A test record.
FEATURES             Location/Qualifiers
     gene            complement(join(100..120,1..10))
                     /gene=\"ori\"
     gene            20..70
                     /gene=\"abc\"
     CDS             join(20..40,50..70)
                     /gene=\"abc\"
ORIGIN
        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt
       61 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt
//
";

fn replace(start: u64, end: u64, residues: &str) -> Op {
    Op::SeqReplace {
        start,
        end,
        residues: residues.into(),
    }
}

fn feature(start: u64, end: u64) -> Feature {
    Feature {
        kind: "misc_feature".into(),
        location: Location::simple(start, end, Strand::Forward),
        qualifiers: vec![Qualifier::new("note", "added")],
    }
}

fn record_session(root: &Path) {
    let path = root.join("seqs/test.gb");
    let mut s = Session::new(root);
    let d = s.open(&path).unwrap();
    // A typing burst: merges into one entry.
    for (i, c) in "GATTACA".chars().enumerate() {
        s.apply(d, replace(15 + i as u64, 15 + i as u64, &c.to_string()))
            .unwrap();
    }
    // A deletion that trims the CDS's first exon.
    s.apply(d, replace(25, 35, "")).unwrap();
    let added = s
        .apply(
            d,
            Op::FeatureAdd {
                feature: feature(5, 30),
            },
        )
        .unwrap();
    let fid = added.feature.unwrap().0;
    s.apply(
        d,
        Op::FeatureUpdate {
            id: fid,
            feature: feature(6, 31),
        },
    )
    .unwrap();
    s.undo(d).unwrap();
    s.undo(d).unwrap();
    s.redo(d).unwrap();
    s.apply(d, Op::FeatureRemove { id: 0 }).unwrap();
    s.undo(d).unwrap();
    s.apply(d, replace(0, 3, "NNNN")).unwrap();
    s.extract(d, 15, 50, &root.join("seqs/part.gb")).unwrap();
    s.save(d, None).unwrap();
}

#[test]
fn recorded_session_replays_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    record_session(root);

    let entries = basecraft_ops::log::read_all(root).unwrap();
    let inserts = entries
        .iter()
        .filter(|e| matches!(&e.op, Op::SeqReplace { start: 15, .. }))
        .count();
    assert_eq!(inserts, 1, "typing burst should be one entry");

    // The original was overwritten by the save; replay uses the stored copy.
    let report = replay::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.sessions, 1);
    assert_eq!(report.entries, entries.len());
}

#[test]
fn extract_clips_annotations_and_marks_cuts_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    let mut s = Session::new(root);
    let d = s.open(&root.join("seqs/test.gb")).unwrap();
    // CDS join(20..40,50..70); extract 1-based 30..60.
    s.extract(d, 29, 60, &root.join("seqs/part.gb")).unwrap();
    let text = std::fs::read_to_string(root.join("seqs/part.gb")).unwrap();
    let rec = &basecraft_io::genbank::parse(text.as_bytes()).unwrap()[0];
    assert_eq!(rec.sequence.len(), 31);
    let cds = rec.features.iter().find(|f| f.kind == "CDS").unwrap();
    assert_eq!(
        basecraft_io::genbank::format_location(&cds.location),
        "join(<1..11,21..>31)"
    );
    assert!(!rec.circular);
}

#[test]
fn replay_detects_divergence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    record_session(root);

    // Tamper with one logged deletion.
    let log_dir = basecraft_ops::log::log_dir(root);
    let host = std::fs::read_dir(&log_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let file = std::fs::read_dir(&host)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = std::fs::read_to_string(&file).unwrap();
    let tampered = text.replace(r#""start":25,"end":35"#, r#""start":25,"end":36"#);
    assert_ne!(text, tampered);
    std::fs::write(&file, tampered).unwrap();

    let report = replay::replay(root).unwrap();
    assert!(!report.ok());
}

#[test]
fn fasta_with_gff_sidecar_saves_and_replays() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("g.fasta"),
        ">chr1 test\nACGTACGTAC\nACGTACGTAC\n>chr2\nGGGG\n",
    )
    .unwrap();
    std::fs::write(
        root.join("g.gff3"),
        "##gff-version 3\n##sequence-region chr1 1 20\nchr1\t.\tgene\t5\t15\t.\t+\t.\tID=g1\n",
    )
    .unwrap();
    {
        let mut s = Session::new(root);
        let d = s.open(&root.join("g.fasta")).unwrap();
        s.apply(d, replace(0, 0, "TT")).unwrap();
        s.save(d, None).unwrap();
    }
    let saved = std::fs::read_to_string(root.join("g.fasta")).unwrap();
    assert_eq!(saved, ">chr1 test\nTTACGTACGTACACGTACGTAC\n>chr2\nGGGG\n");
    let gff = std::fs::read_to_string(root.join("g.gff3")).unwrap();
    assert!(gff.contains("chr1\t.\tgene\t7\t17\t"), "{gff}");
    let report = replay::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}
