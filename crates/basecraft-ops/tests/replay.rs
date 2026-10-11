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
    s.extract(d, (15, 50), &[], false, &root.join("seqs/part.gb"))
        .unwrap();
    // The CDS spliced, and the reverse-strand ori gene reverse-complemented.
    s.extract(
        d,
        (19, 70),
        &[(19, 40), (49, 70)],
        false,
        &root.join("seqs/cds.gb"),
    )
    .unwrap();
    s.extract(d, (90, 110), &[], true, &root.join("seqs/rc.gb"))
        .unwrap();
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
    s.extract(d, (29, 60), &[], false, &root.join("seqs/part.gb"))
        .unwrap();
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
fn extracts_spliced_cds_and_reverse_complements() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    let mut s = Session::new(root);
    let d = s.open(&root.join("seqs/test.gb")).unwrap();
    let original = s.doc(d).read(0, 120);

    // Spliced CDS join(20..40,50..70): 42 bases, one CDS over all of them.
    s.extract(
        d,
        (19, 70),
        &[(19, 40), (49, 70)],
        false,
        &root.join("seqs/cds.gb"),
    )
    .unwrap();
    let rec = &basecraft_io::genbank::parse(&std::fs::read(root.join("seqs/cds.gb")).unwrap())
        .unwrap()[0];
    let mut want = original[19..40].to_vec();
    want.extend_from_slice(&original[49..70]);
    assert_eq!(rec.sequence, want);
    let cds = rec.features.iter().find(|f| f.kind == "CDS").unwrap();
    assert_eq!(
        basecraft_io::genbank::format_location(&cds.location),
        "1..42"
    );
    assert_eq!(rec.name, "TEST_abc");

    // A reverse-strand range comes out reverse-complemented, its
    // annotation on the forward strand.
    s.apply(
        d,
        Op::FeatureAdd {
            feature: Feature {
                kind: "misc_feature".into(),
                location: basecraft_protocol::Location::simple(79, 90, Strand::Reverse),
                qualifiers: vec![],
            },
        },
    )
    .unwrap();
    s.extract(d, (79, 90), &[], true, &root.join("seqs/rc.gb"))
        .unwrap();
    let rec =
        &basecraft_io::genbank::parse(&std::fs::read(root.join("seqs/rc.gb")).unwrap()).unwrap()[0];
    let rc: Vec<u8> = original[79..90]
        .iter()
        .rev()
        .map(|&b| match b {
            b'A' => b'T',
            b'T' => b'A',
            b'C' => b'G',
            _ => b'C',
        })
        .collect();
    assert_eq!(rec.sequence, rc);
    let f = rec
        .features
        .iter()
        .find(|f| f.kind == "misc_feature")
        .unwrap();
    assert_eq!(basecraft_io::genbank::format_location(&f.location), "1..11");
}

#[test]
fn grouped_operations_undo_together_and_replay() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    {
        let mut s = Session::new(root);
        let d = s.open(&root.join("seqs/test.gb")).unwrap();
        let before = s.doc(d).feature_count();
        s.begin_group();
        for i in 0..3 {
            s.apply(
                d,
                Op::FeatureAdd {
                    feature: feature(i * 10, i * 10 + 5),
                },
            )
            .unwrap();
        }
        s.end_group();
        s.apply(
            d,
            Op::FeatureAdd {
                feature: feature(50, 60),
            },
        )
        .unwrap();
        s.undo(d).unwrap();
        assert_eq!(
            s.doc(d).feature_count(),
            before + 3,
            "the single op undoes alone"
        );
        s.undo(d).unwrap();
        assert_eq!(s.doc(d).feature_count(), before, "the group undoes as one");
        s.redo(d).unwrap();
        assert_eq!(s.doc(d).feature_count(), before + 3);
    }
    let report = replay::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
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

const ALN: &str = ">a first row\nAC-GT-A--\n>b\nAC-GTTA--\n>c\n--CGT-AC-\n>d\nACCGT-A--\n";

fn shift(rows: &[(u32, u32)], start: u64, end: u64, by: i64) -> Op {
    Op::AlnShift {
        rows: rows.to_vec(),
        start,
        end,
        by,
    }
}

fn rows_text(s: &Session, d: basecraft_ops::DocId) -> Vec<String> {
    let a = s.alignment(d);
    (0..a.row_count())
        .map(|r| {
            format!(
                "{} {}",
                a.row(r).name,
                String::from_utf8(a.read(r, 0, a.width())).unwrap()
            )
        })
        .collect()
}

/// Every alignment operation type, with merged bursts, undo and redo, then
/// a save; replay must reproduce each step.
#[test]
fn alignment_session_replays_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("x.aln.fasta"), ALN).unwrap();
    {
        let mut s = Session::new(root);
        let d = s.open(&root.join("x.aln.fasta")).unwrap();
        assert!(s.is_alignment(d));
        assert_eq!(s.alignment(d).width(), 9);
        // Typing over residues along row 0: one entry.
        for (i, c) in "TT".chars().enumerate() {
            s.apply(
                d,
                Op::AlnReplace {
                    row: 0,
                    start: 3 + i as u64,
                    end: 4 + i as u64,
                    residues: c.to_string(),
                },
            )
            .unwrap();
        }
        // Typing in insert mode along row 3: one entry.
        for (i, c) in "GA".chars().enumerate() {
            s.apply(
                d,
                Op::AlnInsert {
                    rows: vec![(3, 4)],
                    col: 2 + i as u64,
                    residues: c.to_string(),
                },
            )
            .unwrap();
        }
        // A letter typed into a block in insert mode, then gaps typed over it.
        s.apply(
            d,
            Op::AlnInsert {
                rows: vec![(0, 2)],
                col: 1,
                residues: "CC".into(),
            },
        )
        .unwrap();
        s.apply(
            d,
            Op::AlnFill {
                rows: vec![(0, 2)],
                start: 1,
                end: 3,
                residue: "-".into(),
            },
        )
        .unwrap();
        // Two gaps typed one after the other in rows 1..3: one entry.
        for i in 0..2 {
            s.apply(
                d,
                Op::AlnInsertGaps {
                    rows: vec![(1, 3)],
                    col: 2 + i,
                    count: 1,
                },
            )
            .unwrap();
        }
        // A block slid left in two steps: one entry.
        s.apply(d, shift(&[(2, 3)], 4, 6, -1)).unwrap();
        s.apply(d, shift(&[(2, 3)], 3, 5, -1)).unwrap();
        // Residues in the way.
        assert!(s.apply(d, shift(&[(0, 1)], 1, 3, 1)).is_err());
        s.apply(
            d,
            Op::AlnDelete {
                rows: vec![(0, 4)],
                start: 0,
                end: 1,
            },
        )
        .unwrap();
        s.apply(
            d,
            Op::AlnMoveRows {
                rows: vec![(3, 4)],
                to: 0,
            },
        )
        .unwrap();
        s.apply(
            d,
            Op::AlnRenameRow {
                row: 0,
                name: "dee".into(),
            },
        )
        .unwrap();
        s.apply(d, Op::AlnRemoveRows { rows: vec![(2, 3)] })
            .unwrap();
        s.undo(d).unwrap();
        s.undo(d).unwrap();
        s.redo(d).unwrap();
        s.apply(d, Op::AlnRemoveGapColumns {}).unwrap();
        // A tool run that wrote a new file.
        std::fs::write(root.join("x.mafft.fasta"), ">a\nAC\n>b\nAC\n").unwrap();
        s.log_tool_run(
            d,
            basecraft_protocol::Actor::Tool {
                tool: "mafft".into(),
                version: "v7.526".into(),
            },
            ("basecraft.mafft", "align"),
            serde_json::json!({}),
            &[("x.aln.fasta (ungapped)".into(), "b3:00".into())],
            &[root.join("x.mafft.fasta")],
        )
        .unwrap();
        // Sequence operations don't apply to an alignment.
        assert!(s.apply(d, replace(0, 0, "A")).is_err());
        s.save(d, None).unwrap();
        let want = rows_text(&s, d);
        let reopened = {
            let mut s2 = Session::unlogged(root);
            let d2 = s2.open(&root.join("x.aln.fasta")).unwrap();
            rows_text(&s2, d2)
        };
        assert_eq!(reopened, want);
    }
    let entries = basecraft_ops::log::read_all(root).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.op.name()).collect();
    assert_eq!(
        names.iter().filter(|n| **n == "aln.replace").count(),
        1,
        "{names:?}"
    );
    assert_eq!(names.iter().filter(|n| **n == "aln.insert_gaps").count(), 1);
    // Typing along row 3, and the letter typed into a block.
    assert_eq!(names.iter().filter(|n| **n == "aln.insert").count(), 2);
    assert_eq!(names.iter().filter(|n| **n == "aln.fill").count(), 1);
    assert_eq!(names.iter().filter(|n| **n == "tool.run").count(), 1);
    assert_eq!(names.iter().filter(|n| **n == "aln.shift").count(), 1);
    let saved = std::fs::read_to_string(root.join("x.aln.fasta")).unwrap();
    assert!(saved.starts_with(">dee\n"), "{saved}");
    assert!(saved.contains(">a first row\n"), "{saved}");
    let report = replay::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.entries, entries.len());
}

#[test]
fn multi_record_fasta_opens_as_alignment() {
    use basecraft_ops::files::Kind;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("set.fasta"), ">a\nACGT\n>b\nACG\n").unwrap();
    std::fs::write(root.join("annotated.fasta"), ">a\nACGT\n>b\nACG\n").unwrap();
    std::fs::write(root.join("annotated.gff3"), "##gff-version 3\n").unwrap();
    let chromosome = "A".repeat(1_000_001);
    std::fs::write(
        root.join("genome.fasta"),
        format!(">c1\n{chromosome}\n>c2\nACGT\n"),
    )
    .unwrap();
    std::fs::write(root.join("aln.fasta"), ">a\nAC-T\n>b\nACGT\n").unwrap();
    std::fs::write(root.join("ragged.afa"), ">a\nAC\n>b\nACGT\n").unwrap();
    let kind = |f: &str| basecraft_ops::files::kind(&root.join(f)).unwrap();
    assert_eq!(kind("set.fasta"), Kind::Alignment);
    assert_eq!(kind("annotated.fasta"), Kind::Sequence);
    assert_eq!(kind("genome.fasta"), Kind::Sequence);
    assert_eq!(kind("aln.fasta"), Kind::Alignment);
    assert_eq!(kind("ragged.afa"), Kind::Alignment);
    let mut s = Session::unlogged(root);
    let d = s.open(&root.join("ragged.afa")).unwrap();
    assert_eq!(rows_text(&s, d), ["a AC--", "b ACGT"]);
}
