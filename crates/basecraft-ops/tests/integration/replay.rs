// Record sessions, then replay the log and check every step.

use std::path::Path;

use basecraft_ops::{Session, records};
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
    Op::seq_replace(start, end, residues)
}

/// A plugin's offer of `operations` against the document as it is now.
fn offer<'a>(
    session: &Session,
    doc: basecraft_ops::DocId,
    operations: &'a [Op],
    actor: &'a basecraft_protocol::Actor,
) -> basecraft_ops::session::SequenceOffer<'a> {
    basecraft_ops::session::SequenceOffer {
        generation: session.generation(doc).unwrap(),
        revision: session.doc(doc).unwrap().version(),
        operations,
        actor,
        source: basecraft_protocol::ops::ProposalSource::Run { id: "run-1".into() },
        skip_known: false,
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
    // A typing burst: one undo step, one journal entry per keystroke.
    for (i, c) in "GATTACA".chars().enumerate() {
        s.apply(d, replace(15 + i as u64, 15 + i as u64, &c.to_string()))
            .unwrap();
    }
    // A deletion that trims the CDS's first exon.
    s.apply(d, replace(25, 35, "")).unwrap();
    let added = s.apply(d, Op::feature_add(feature(5, 30))).unwrap();
    let fid = added.feature.unwrap().0;
    s.apply(d, Op::feature_update(fid, feature(6, 31))).unwrap();
    s.undo(d).unwrap();
    s.undo(d).unwrap();
    s.redo(d).unwrap();
    s.apply(d, Op::feature_remove(0)).unwrap();
    s.undo(d).unwrap();
    s.apply(d, replace(0, 3, "NNNN")).unwrap();
    s.extract(d, (15, 50), &[], false, &root.join("seqs/part.gb"), None)
        .unwrap();
    // The CDS spliced, and the reverse-strand ori gene reverse-complemented.
    s.extract(
        d,
        (19, 70),
        &[(19, 40), (49, 70)],
        false,
        &root.join("seqs/cds.gb"),
        None,
    )
    .unwrap();
    s.extract(d, (90, 110), &[], true, &root.join("seqs/rc.gb"), None)
        .unwrap();
    s.save(d, None).unwrap();
}

#[test]
fn recorded_session_replays_exactly() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    record_session(root);

    let entries = basecraft_ops::log::read_all(root).unwrap();
    let inserts = entries
        .iter()
        .filter(|e| matches!(&e.op, Op::SeqReplace { start, end, .. } if (15..22).contains(start) && start == end))
        .count();
    assert_eq!(inserts, 7, "each keystroke is journaled");

    // The save replaced the original: replay restarts at the save, and the
    // entries before it are not checkable.
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.sessions, 1);
    assert_eq!(report.entries, entries.len());
    let save = entries
        .iter()
        .position(|e| matches!(e.op, Op::Save { .. }))
        .unwrap();
    assert_eq!(report.unchecked, save);
    // With the original back, every entry is checked.
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.unchecked, 0);
}

#[test]
fn extract_clips_annotations_and_marks_cuts_incomplete() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    let mut s = Session::new(root);
    let d = s.open(&root.join("seqs/test.gb")).unwrap();
    // CDS join(20..40,50..70); extract 1-based 30..60.
    s.extract(d, (29, 60), &[], false, &root.join("seqs/part.gb"), None)
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
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    let mut s = Session::new(root);
    let d = s.open(&root.join("seqs/test.gb")).unwrap();
    let original = s.doc(d).unwrap().read(0, 120);

    // Spliced CDS join(20..40,50..70): 42 bases, one CDS over all of them.
    s.extract(
        d,
        (19, 70),
        &[(19, 40), (49, 70)],
        false,
        &root.join("seqs/cds.gb"),
        None,
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
        Op::feature_add(Feature {
            kind: "misc_feature".into(),
            location: basecraft_protocol::Location::simple(79, 90, Strand::Reverse),
            qualifiers: vec![],
        }),
    )
    .unwrap();
    s.extract(d, (79, 90), &[], true, &root.join("seqs/rc.gb"), None)
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
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("seqs")).unwrap();
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();
    {
        let mut s = Session::new(root);
        let d = s.open(&root.join("seqs/test.gb")).unwrap();
        let before = s.doc(d).unwrap().feature_count();
        s.begin_group();
        for i in 0..3 {
            s.apply(d, Op::feature_add(feature(i * 10, i * 10 + 5)))
                .unwrap();
        }
        s.end_group();
        s.apply(d, Op::feature_add(feature(50, 60))).unwrap();
        s.undo(d).unwrap();
        assert_eq!(
            s.doc(d).unwrap().feature_count(),
            before + 3,
            "the single op undoes alone"
        );
        s.undo(d).unwrap();
        assert_eq!(
            s.doc(d).unwrap().feature_count(),
            before,
            "the group undoes as one"
        );
        s.redo(d).unwrap();
        assert_eq!(s.doc(d).unwrap().feature_count(), before + 3);
    }
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}

#[test]
fn replay_detects_divergence() {
    crate::isolated_profile::isolate!();
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
    // Recompute integrity to exercise semantic divergence independently of corruption.
    let tampered = text
        .lines()
        .map(|line| {
            let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
            value.as_object_mut().unwrap().remove("_checksum");
            let sequence = value.as_object_mut().unwrap().remove("_sequence").unwrap();
            let writer = value.as_object_mut().unwrap().remove("basecraft").unwrap();
            if value["params"]["start"] == 25 && value["params"]["end"] == 35 {
                value["params"]["end"] = serde_json::json!(36);
            }
            let checksum = basecraft_io::hash_bytes(&serde_json::to_vec(&value).unwrap());
            value["_checksum"] = serde_json::json!(checksum);
            value["_sequence"] = sequence;
            value["basecraft"] = writer;
            serde_json::to_string(&value).unwrap() + "\n"
        })
        .collect::<String>();
    assert_ne!(text, tampered);
    std::fs::write(&file, tampered).unwrap();
    // The save replaced the original; put it back so the edits are checked.
    std::fs::write(root.join("seqs/test.gb"), GB).unwrap();

    let report = records::replay(root).unwrap();
    assert!(!report.ok());
}

#[test]
fn fasta_with_gff_sidecar_saves_and_replays() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("g.fasta"), ">chr1 test\nACGTACGTAC\nACGTACGTAC\n").unwrap();
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
    assert_eq!(saved, ">chr1 test\nTTACGTACGTACACGTACGTAC\n");
    let gff = std::fs::read_to_string(root.join("g.gff3")).unwrap();
    assert!(gff.contains("chr1\t.\tgene\t7\t17\t"), "{gff}");
    let report = records::replay(root).unwrap();
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
    let a = s.alignment(d).unwrap();
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
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("x.aln.fasta"), ALN).unwrap();
    {
        let mut s = Session::new(root);
        let d = s.open(&root.join("x.aln.fasta")).unwrap();
        assert!(s.is_alignment(d));
        assert_eq!(s.alignment(d).unwrap().width(), 9);
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
        s.record_tool_run(
            d,
            basecraft_protocol::Actor::Tool {
                tool: "mafft".into(),
                version: "v7.526".into(),
            },
            basecraft_ops::session::ToolRunRecord::new(
                ("basecraft.mafft", "align"),
                serde_json::json!({}),
                &[("x.aln.fasta (ungapped)".into(), "b3:00".into())],
                &[root.join("x.mafft.fasta")],
            ),
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
    // Bursts share undo steps; the journal keeps every keystroke.
    let count = |name: &str| names.iter().filter(|n| **n == name).count();
    assert_eq!(count("aln.replace"), 2, "{names:?}");
    assert_eq!(count("aln.insert_gaps"), 2);
    // Typing along row 3 (two keys), and the letter typed into a block.
    assert_eq!(count("aln.insert"), 3);
    assert_eq!(count("aln.fill"), 1);
    assert_eq!(count("tool.run"), 1);
    assert_eq!(count("aln.shift"), 2);
    let saved = std::fs::read_to_string(root.join("x.aln.fasta")).unwrap();
    assert!(saved.starts_with(">dee\n"), "{saved}");
    assert!(saved.contains(">a first row\n"), "{saved}");
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.entries, entries.len());
}

#[test]
fn multi_record_fasta_opens_as_alignment() {
    use basecraft_ops::files::Kind;
    crate::isolated_profile::isolate!();
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

fn clade(tips: &[&str]) -> Vec<String> {
    tips.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn tree_session_replays_exactly() {
    use basecraft_protocol::{CladeAnnotation, SupportPlacement, TreeStyle};
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("t.nwk"), "(A:1,B:2,(C:3,(D:4,E:5)90:6)80:7);\n").unwrap();
    std::fs::write(
        root.join("other.tre"),
        "#NEXUS\nbegin trees;\ntree con = [&U] ((A,B)[&prob=0.9],C,(D,E)[&prob=1.0]);\nend;\n",
    )
    .unwrap();
    {
        let mut s = Session::new(root);
        let d = s.open(&root.join("t.nwk")).unwrap();
        assert!(s.is_tree(d));
        assert_eq!(s.tree(d).unwrap().tip_count(), 5);
        s.apply(
            d,
            Op::TreeReroot {
                clade: clade(&["D", "E"]),
            },
        )
        .unwrap();
        s.apply(
            d,
            Op::TreeRotate {
                clade: clade(&["A", "C"]),
            },
        )
        .unwrap();
        s.apply(
            d,
            Op::TreeRootPosition {
                clade: clade(&["D", "E"]),
                fraction: 0.3,
            },
        )
        .unwrap();
        s.apply(d, Op::TreeLadderize { small_first: true }).unwrap();
        let mut a = CladeAnnotation::new(clade(&["D", "E"]), "Delta group");
        a.field = true;
        a.opacity_left = 0;
        s.apply(
            d,
            Op::TreeAnnotate {
                id: 1,
                annotation: Some(a),
            },
        )
        .unwrap();
        // Support from a MrBayes-style tree, matched by branch.
        let other = basecraft_ops::files::load_tree(&root.join("other.tre")).unwrap();
        let sets = s.tree(d).unwrap().support_from(&other.doc).unwrap();
        assert_eq!(sets.len(), 1);
        s.apply(
            d,
            Op::TreeAddSupport {
                kind: basecraft_protocol::SupportKind::Probability,
                name: "PP".into(),
                source: "other.tre".into(),
                values: sets[0].1.clone(),
            },
        )
        .unwrap();
        s.apply(
            d,
            Op::TreeRenameTip {
                from: "A".into(),
                to: "Alpha".into(),
            },
        )
        .unwrap();
        let style = TreeStyle {
            support_placement: SupportPlacement::Below,
            ..TreeStyle::default()
        };
        s.apply(
            d,
            Op::TreeStyle {
                style,
                remove_supports: Vec::new(),
            },
        )
        .unwrap();
        // An edit undone, and a bad one refused.
        s.apply(
            d,
            Op::TreeRotate {
                clade: clade(&["D", "E"]),
            },
        )
        .unwrap();
        s.undo(d).unwrap();
        assert!(
            s.apply(
                d,
                Op::TreeRotate {
                    clade: clade(&["Nope"])
                }
            )
            .is_err()
        );
        // A figure: logged with its hash, not redrawn on replay.
        s.export(d, &root.join("t.svg"), "svg", |w| w.write_all(b"<svg/>"))
            .unwrap();
        s.save(d, None).unwrap();
    }
    let saved = std::fs::read_to_string(root.join("t.nwk")).unwrap();
    assert!(saved.contains("Alpha"), "{saved}");
    assert!(saved.contains("90/1"), "{saved}");
    // Reopening restores the set names, annotation and style.
    let reopened = basecraft_ops::files::load_tree(&root.join("t.nwk")).unwrap();
    let names: Vec<&str> = reopened
        .doc
        .supports()
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["Support", "PP"]);
    assert_eq!(reopened.doc.annotations()[&1].label, "Delta group");
    assert_eq!(
        reopened.doc.style().support_placement,
        SupportPlacement::Below
    );
    let entries = basecraft_ops::log::read_all(root).unwrap();
    assert!(entries.iter().any(|e| e.op.name() == "doc.export"));
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}

#[test]
fn sequence_addition_expands_reference_and_replays_with_undo() {
    crate::isolated_profile::isolate!();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let path = root.join("addition.afa");
    std::fs::write(&path, ">a description\nAC-GT\n>b\nAC-GT\n").unwrap();
    {
        let mut session = Session::new(root);
        let doc = session.open(&path).unwrap();
        let before = rows_text(&session, doc);
        assert!(
            session
                .apply(
                    doc,
                    Op::AlnAddSequences {
                        rows: vec![("a".into(), "ACTGT".into())],
                        reference: None,
                    }
                )
                .is_err()
        );
        assert_eq!(rows_text(&session, doc), before);
        session
            .apply(
                doc,
                Op::AlnAddSequences {
                    rows: vec![("new".into(), "AC-GGT".into())],
                    reference: Some(vec!["AC-G-T".into(), "AC-G-T".into()]),
                },
            )
            .unwrap();
        assert_eq!(session.alignment(doc).unwrap().width(), 6);
        assert_eq!(session.alignment(doc).unwrap().row_count(), 3);
        let added = rows_text(&session, doc);
        session.undo(doc).unwrap();
        assert_eq!(rows_text(&session, doc), before);
        session.redo(doc).unwrap();
        assert_eq!(rows_text(&session, doc), added);
        session
            .apply(
                doc,
                Op::AlnAddSequences {
                    rows: vec![("unaligned".into(), "AA".into())],
                    reference: None,
                },
            )
            .unwrap();
        assert_eq!(session.alignment(doc).unwrap().read(3, 0, 6), b"AA----");
        session.save(doc, None).unwrap();
    }
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}

#[test]
fn automatic_root_operations_undo_redo_save_reload_and_replay() {
    crate::isolated_profile::isolate!();
    for op in [Op::TreeMidpointRoot {}, Op::TreeMinimumVarianceRoot {}] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = root.join("root.nwk");
        std::fs::write(&path, "(A:1,B:2,(C:3,D:9)90:4);").unwrap();
        let saved;
        {
            let mut session = Session::new(root);
            let doc = session.open(&path).unwrap();
            let mut annotation =
                basecraft_protocol::CladeAnnotation::new(clade(&["C", "D"]), "edge label");
            annotation.on_branch = true;
            annotation.bar = false;
            session
                .apply(
                    doc,
                    Op::TreeAnnotate {
                        id: 1,
                        annotation: Some(annotation),
                    },
                )
                .unwrap();
            let before = session.tree(doc).unwrap().clone();
            session.apply(doc, op.clone()).unwrap();
            saved = session.tree(doc).unwrap().clone();
            assert_ne!(before.to_newick(), saved.to_newick());
            session.undo(doc).unwrap();
            assert_eq!(session.tree(doc).unwrap().to_newick(), before.to_newick());
            assert_eq!(
                session.tree(doc).unwrap().annotations(),
                before.annotations()
            );
            session.redo(doc).unwrap();
            assert_eq!(session.tree(doc).unwrap().to_newick(), saved.to_newick());
            assert_eq!(
                session.tree(doc).unwrap().annotations(),
                saved.annotations()
            );
            session.save(doc, None).unwrap();
        }
        let reloaded = basecraft_ops::files::load_tree(&path).unwrap().doc;
        assert_eq!(reloaded.to_newick(), saved.to_newick());
        assert_eq!(reloaded.annotations(), saved.annotations());
        assert_eq!(reloaded.supports(), saved.supports());
        let entries = basecraft_ops::log::read_all(root).unwrap();
        assert!(entries.iter().any(|e| e.op.name() == op.name()));
        let report = records::replay(root).unwrap();
        assert!(report.ok(), "{:#?}", report.problems);
    }
}

#[test]
fn automatic_root_error_keeps_document_and_history() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.nwk");
    std::fs::write(&path, "(A,B:2);").unwrap();
    let mut session = Session::new(dir.path());
    let doc = session.open(&path).unwrap();
    let before = session.tree(doc).unwrap().to_newick();
    let entries = basecraft_ops::log::read_all(dir.path()).unwrap().len();
    for op in [Op::TreeMidpointRoot {}, Op::TreeMinimumVarianceRoot {}] {
        assert!(session.apply(doc, op).is_err());
        assert_eq!(session.tree(doc).unwrap().to_newick(), before);
        assert!(!session.can_undo(doc).unwrap());
        assert_eq!(
            basecraft_ops::log::read_all(dir.path()).unwrap().len(),
            entries
        );
    }
}

#[test]
fn strict_sequence_proposals_stage_invalid_batches_and_replay_one_undo_group() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sequence.fa");
    std::fs::write(&path, ">sequence\nACGTACGT\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&path).unwrap();
    let generation = session.generation(doc).unwrap();
    let actor = basecraft_protocol::Actor::Plugin {
        id: "org.example.annotation".into(),
        version: "1.0.0".into(),
    };
    let before = session.doc(doc).unwrap().read(0, 8);
    let invalid = [replace(0, 1, "T"), replace(0, 99, "A")];
    assert!(
        session
            .apply_sequence_proposals(doc, offer(&session, doc, &invalid, &actor))
            .is_err()
    );
    assert_eq!(session.doc(doc).unwrap().read(0, 8), before);
    assert_eq!(session.generation(doc).unwrap(), generation);
    assert!(!session.can_undo(doc).unwrap());
    let one = [replace(0, 1, "T")];
    let mut stale = offer(&session, doc, &one, &actor);
    stale.generation += 1;
    assert!(session.apply_sequence_proposals(doc, stale).is_err());
    let unsafe_save = [Op::Save {
        path: "unsafe.fa".into(),
    }];
    assert!(
        session
            .apply_sequence_proposals(doc, offer(&session, doc, &unsafe_save, &actor))
            .is_err()
    );
    assert_eq!(session.generation(doc).unwrap(), generation);
    let ops = [replace(0, 1, "T"), Op::feature_add(feature(1, 4))];
    session
        .apply_sequence_proposals(doc, offer(&session, doc, &ops, &actor))
        .unwrap();
    assert_eq!(session.doc(doc).unwrap().read(0, 8), b"TCGTACGT");
    assert_eq!(session.doc(doc).unwrap().features().count(), 1);
    session.undo(doc).unwrap();
    assert_eq!(session.doc(doc).unwrap().read(0, 8), before);
    assert_eq!(session.doc(doc).unwrap().features().count(), 0);
    session.redo(doc).unwrap();
    assert_eq!(session.doc(doc).unwrap().read(0, 8), b"TCGTACGT");
    assert!(records::replay(temp.path()).unwrap().ok());
    assert_eq!(std::fs::read(&path).unwrap(), b">sequence\nACGTACGT\n");
}

#[test]
fn script_actor_edits_read_back_replay_and_show_as_script() {
    use basecraft_protocol::Actor;
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("g.fasta"), ">g\nACGTACGT\n").unwrap();
    let script = Actor::Script {
        client: "Python".into(),
    };
    let agent = Actor::Agent {
        model: "test-agent".into(),
    };
    {
        let mut s = Session::new(root);
        s.set_actor(script.clone());
        let d = s.open(&root.join("g.fasta")).unwrap();
        s.apply(d, replace(0, 1, "T")).unwrap();
        // An older-style agent edit in the same history stays an agent edit.
        s.apply_as(d, replace(4, 5, "G"), agent.clone()).unwrap();
        s.save(d, None).unwrap();
    }
    let entries = basecraft_ops::log::read_all(root).unwrap();
    let actor_of = |start: u64| {
        entries
            .iter()
            .find(|e| matches!(&e.op, Op::SeqReplace { start: s, .. } if *s == start))
            .map(|e| e.actor.clone())
            .unwrap()
    };
    assert_eq!(actor_of(0), script);
    assert_eq!(actor_of(4), agent);
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.entries, entries.len());

    let activity = basecraft_ops::log::activity_text(&[root.to_path_buf()], "", 0).unwrap();
    assert!(activity.contains("[script Python]"), "{activity}");
    assert!(activity.contains("[agent test-agent]"), "{activity}");
    let mut reader = Session::new(root);
    let d = reader.open(&root.join("g.fasta")).unwrap();
    let record = reader.document_record(d).unwrap().report().unwrap();
    let details: Vec<&str> = record
        .history
        .iter()
        .filter(|row| row.summary.to_lowercase().contains("replace"))
        .map(|row| row.details.as_str())
        .collect();
    assert!(
        details.iter().any(|d| d.starts_with("Script: Python")),
        "{details:#?}"
    );
    assert!(
        details.iter().any(|d| d.starts_with("Agent: test-agent")),
        "{details:#?}"
    );
    assert!(!details.iter().any(|d| d.starts_with("Agent: Python")));
}

#[test]
fn submission_edits_undo_save_and_replay() {
    use basecraft_protocol::submission::{
        BatchSettings, Decision, FrameChoice, ItsComponent, Marker, RowDecision, SubmissionEdit,
    };
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("its.afa"), ">a\nACGTACGT\n>b\nACGTAC--\n").unwrap();
    {
        let mut s = Session::new(root);
        let doc = basecraft_ops::submission::create(
            &mut s,
            Path::new("its.submission.json"),
            Path::new("its.afa"),
        )
        .unwrap();
        let edits = [
            SubmissionEdit::SetBatch {
                batch: BatchSettings {
                    marker: Marker::Its {
                        components: vec![ItsComponent::Its1, ItsComponent::Rrna58S],
                    },
                    ..BatchSettings::default()
                },
            },
            SubmissionEdit::SetRows {
                rows: vec![RowDecision {
                    record: "b".into(),
                    decision: Decision::Exclude,
                }],
            },
            SubmissionEdit::RecordExport {
                mapping: basecraft_protocol::submission::ExportMapping {
                    source: s.submission(doc).unwrap().source.clone(),
                    records: [("a".into(), "a".into())].into(),
                },
            },
            SubmissionEdit::SetFrames {
                frames: vec![FrameChoice {
                    record: "a".into(),
                    codon_start: 3,
                }],
                clear: vec![],
            },
        ];
        for edit in edits {
            s.apply(
                doc,
                Op::SubmissionEdit {
                    edit: Box::new(edit),
                },
            )
            .unwrap();
        }
        s.undo(doc).unwrap();
        assert_eq!(s.submission(doc).unwrap().frames, Vec::new());
        s.save(doc, None).unwrap();
        s.redo(doc).unwrap();
        s.save(doc, None).unwrap();
    }
    let report = records::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}

/// The journal's entries after the documents were opened.
fn proposal_entries(root: &Path) -> Vec<basecraft_protocol::LogEntry> {
    basecraft_ops::log::read_all(root)
        .unwrap()
        .into_iter()
        .filter(|e| !matches!(e.op, Op::Open { .. }))
        .collect()
}

fn annotation_plugin() -> basecraft_protocol::Actor {
    basecraft_protocol::Actor::Plugin {
        id: "org.example.annotation".into(),
        version: "1.0.0".into(),
    }
}

#[test]
fn accepted_proposal_logs_offer_edits_and_decision_and_shows_one_row() {
    use basecraft_protocol::ops::{ProposalDecision, ProposalSource};
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sequence.fa");
    std::fs::write(&path, ">sequence\nACGTACGTACGT\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&path).unwrap();
    let actor = annotation_plugin();
    let ops = [
        Op::feature_add(feature(0, 4)),
        Op::feature_add(feature(4, 8)),
    ];
    let applied = session
        .apply_sequence_proposals(doc, offer(&session, doc, &ops, &actor))
        .unwrap()
        .applied;
    let entries = proposal_entries(temp.path());
    let names: Vec<_> = entries.iter().map(|e| e.op.name()).collect();
    assert_eq!(
        names,
        [
            "proposal.offer",
            "feature.add",
            "feature.add",
            "proposal.decide"
        ]
    );
    let Op::ProposalOffer {
        proposal,
        source,
        base_revision,
        operations,
    } = &entries[0].op
    else {
        unreachable!()
    };
    assert_eq!(source, &ProposalSource::Run { id: "run-1".into() });
    assert_eq!(*base_revision, 0);
    assert_eq!(operations[..], ops[..]);
    assert_eq!(entries[0].actor, actor);
    assert_eq!(entries[0].state.features, 0);
    assert_eq!(
        entries[3].op,
        Op::ProposalDecide {
            proposal: proposal.clone(),
            outcome: ProposalDecision::Accepted,
            automatic: true,
            applied: applied.clone(),
            note: None,
        }
    );
    assert_eq!(entries[3].state.features, 2);
    assert_eq!(applied, [entries[1].id.clone(), entries[2].id.clone()]);
    assert!(records::replay(temp.path()).unwrap().ok());

    let report = session.document_record(doc).unwrap().report().unwrap();
    let rows: Vec<_> = report
        .history
        .iter()
        .filter(|row| !row.summary.starts_with("Opened"))
        .collect();
    assert_eq!(rows.len(), 1, "{:#?}", report.history);
    assert_eq!(
        rows[0].summary,
        "org.example.annotation proposed 2 features; all accepted automatically"
    );
    assert_eq!(rows[0].status, "Contributing");
    assert_eq!(rows[0].id, entries[0].id);
    // Methods describe the edits applied, as before.
    assert_eq!(report.paragraphs.len(), 2);

    // Undone, the proposal stays as a row and stops contributing.
    session.undo(doc).unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    let row = report
        .history
        .iter()
        .find(|row| row.id == entries[0].id)
        .unwrap();
    assert_eq!(row.status, "Undone / not contributing");
    assert!(report.paragraphs.is_empty());
    assert!(records::replay(temp.path()).unwrap().ok());

    // Redone, it contributes again.
    session.redo(doc).unwrap();
    let report = session.document_record(doc).unwrap().report().unwrap();
    let row = report
        .history
        .iter()
        .find(|row| row.id == entries[0].id)
        .unwrap();
    assert_eq!(row.status, "Contributing");
    assert_eq!(report.paragraphs.len(), 2);
    assert!(records::replay(temp.path()).unwrap().ok());
}

#[test]
fn proposal_leaving_out_known_features_is_recorded_as_modified() {
    use basecraft_protocol::ops::ProposalDecision;
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sequence.fa");
    std::fs::write(&path, ">sequence\nACGTACGTACGT\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&path).unwrap();
    session.apply(doc, Op::feature_add(feature(0, 4))).unwrap();
    let actor = annotation_plugin();
    let ops = [
        Op::feature_add(feature(0, 4)),
        Op::feature_add(feature(4, 8)),
    ];
    let mut offered = offer(&session, doc, &ops, &actor);
    offered.skip_known = true;
    let result = session.apply_sequence_proposals(doc, offered).unwrap();
    assert_eq!(result.applied.len(), 1);
    assert_eq!(result.skipped, ["already annotated: misc_feature"]);
    let entries = proposal_entries(temp.path());
    let decide = entries.last().unwrap();
    let Op::ProposalDecide {
        outcome,
        applied,
        note,
        ..
    } = &decide.op
    else {
        panic!("{:?}", decide.op)
    };
    assert_eq!(*outcome, ProposalDecision::Modified);
    assert_eq!(applied, &result.applied);
    assert_eq!(
        note.as_deref(),
        Some("left out already annotated: misc_feature")
    );
    assert!(records::replay(temp.path()).unwrap().ok());
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(report.history.iter().any(|row| row.summary
        == "org.example.annotation proposed 2 features; 1 of 2 accepted automatically \
            (left out already annotated: misc_feature)"));
}

#[test]
fn stale_proposal_is_recorded_as_rejected_and_changes_nothing() {
    use basecraft_protocol::ops::ProposalDecision;
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sequence.fa");
    std::fs::write(&path, ">sequence\nACGTACGT\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&path).unwrap();
    let actor = annotation_plugin();
    let ops = [Op::feature_add(feature(0, 4))];
    let read = offer(&session, doc, &ops, &actor);
    // The user edits while the plugin works.
    session.apply(doc, replace(0, 1, "T")).unwrap();
    let error = session.apply_sequence_proposals(doc, read).unwrap_err();
    assert!(matches!(error, basecraft_ops::Error::StaleRevision { .. }));
    assert_eq!(session.doc(doc).unwrap().features().count(), 0);
    let entries = proposal_entries(temp.path());
    let names: Vec<_> = entries.iter().map(|e| e.op.name()).collect();
    assert_eq!(names, ["seq.replace", "proposal.offer", "proposal.decide"]);
    let Op::ProposalDecide {
        outcome,
        applied,
        note,
        ..
    } = &entries[2].op
    else {
        unreachable!()
    };
    assert_eq!(*outcome, ProposalDecision::Rejected);
    assert_eq!(applied, &Vec::<String>::new());
    assert_eq!(note.as_deref(), Some(error.to_string().as_str()));
    assert!(records::replay(temp.path()).unwrap().ok());
    let report = session.document_record(doc).unwrap().report().unwrap();
    let row = report
        .history
        .iter()
        .find(|row| row.id == entries[1].id)
        .unwrap();
    assert_eq!(row.status, "Rejected");
    assert!(
        row.summary
            .starts_with("org.example.annotation proposed 1 feature; rejected automatically ("),
        "{}",
        row.summary
    );
    // One undo reverts the user's edit; the rejection is not an undo step.
    session.undo(doc).unwrap();
    assert!(!session.can_undo(doc).unwrap());
}

#[test]
fn offer_of_known_features_only_is_rejected_and_keeps_redo() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sequence.fa");
    std::fs::write(&path, ">sequence\nACGTACGT\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&path).unwrap();
    session.apply(doc, Op::feature_add(feature(0, 4))).unwrap();
    session.apply(doc, replace(4, 5, "T")).unwrap();
    session.undo(doc).unwrap();
    let actor = annotation_plugin();
    let ops = [Op::feature_add(feature(0, 4))];
    let mut offered = offer(&session, doc, &ops, &actor);
    offered.skip_known = true;
    let result = session.apply_sequence_proposals(doc, offered).unwrap();
    assert_eq!(result.applied, Vec::<String>::new());
    assert_eq!(result.skipped, ["already annotated: misc_feature"]);
    assert!(session.can_redo(doc).unwrap());
    let names: Vec<_> = proposal_entries(temp.path())
        .iter()
        .map(|e| e.op.name())
        .collect();
    assert_eq!(
        names[names.len() - 2..],
        ["proposal.offer", "proposal.decide"]
    );
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(report.history.iter().any(|row| row.status == "Rejected"
        && row.summary
            == "org.example.annotation proposed 1 feature; rejected automatically \
                (left out already annotated: misc_feature)"));
    session.redo(doc).unwrap();
    assert_eq!(session.doc(doc).unwrap().read(0, 8), b"ACGTTCGT");
    assert!(records::replay(temp.path()).unwrap().ok());
}

#[test]
fn too_many_edits_are_refused_before_staleness_and_not_logged() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sequence.fa");
    std::fs::write(&path, ">sequence\nACGTACGT\n").unwrap();
    let mut session = Session::new(temp.path());
    let doc = session.open(&path).unwrap();
    let actor = annotation_plugin();
    let ops = vec![Op::feature_add(feature(0, 4)); 4097];
    let mut stale = offer(&session, doc, &ops, &actor);
    stale.generation += 1;
    let before = basecraft_ops::log::read_all(temp.path()).unwrap();
    let error = session.apply_sequence_proposals(doc, stale).unwrap_err();
    assert!(
        matches!(error, basecraft_ops::Error::Limit { .. }),
        "{error}"
    );
    assert_eq!(basecraft_ops::log::read_all(temp.path()).unwrap(), before);
}

/// A gene call with `qualifiers`, 20 bp after the previous one.
fn gene_call(index: u64, qualifiers: Vec<Qualifier>) -> Op {
    let start = index * 20;
    Op::feature_add(Feature {
        kind: "CDS".into(),
        location: Location::simple(start, start + 993, Strand::Forward),
        qualifiers,
    })
}

/// A CDS as the bundled Pyrodigal plugin reports it.
fn pyrodigal_gene(index: u64) -> Op {
    gene_call(
        index,
        vec![
            Qualifier::new("label", format!("orf{index}")),
            Qualifier::new("transl_table", "11"),
            Qualifier::new("note", "predicted by Prodigal 2.6.3"),
        ],
    )
}

/// A CDS as a fuller annotation pipeline reports it, with a locus tag,
/// product, inference, a score note and a 500-residue translation: about
/// 9.5 MiB for 4,096 with the offer, where the edits alone take 5.7 MiB.
fn translated_gene(index: u64) -> Op {
    gene_call(
        index,
        vec![
            Qualifier::new("locus_tag", format!("CONTIG_{index:05}")),
            Qualifier::new("product", "hypothetical protein"),
            Qualifier::new("inference", "ab initio prediction:Prodigal:2.6"),
            Qualifier::new(
                "note",
                "ID=1_1;partial=00;start_type=ATG;rbs_motif=GGAG/GAGG;rbs_spacer=5-10bp;\
                 gc_cont=0.512;conf=99.99;score=86.31;cscore=74.92;sscore=11.39",
            ),
            Qualifier::new(
                "translation",
                &"MKLVINGKTLEGEAVSAEELAKRLGVPVA".repeat(18)[..500],
            ),
        ],
    )
}

/// A session on a 100 kb genome.
fn genome_session(temp: &Path) -> (Session, basecraft_ops::DocId) {
    let path = temp.join("genome.fa");
    std::fs::write(&path, format!(">genome\n{}\n", "ACGT".repeat(25_000))).unwrap();
    let mut session = Session::new(temp);
    let doc = session.open(&path).unwrap();
    (session, doc)
}

fn journal_bytes(root: &Path) -> u64 {
    std::fs::read_dir(basecraft_ops::log::log_dir(root))
        .unwrap()
        .flat_map(|host| std::fs::read_dir(host.unwrap().path()).unwrap())
        .map(|file| file.unwrap().metadata().unwrap().len())
        .sum()
}

/// The proposal limit is 4,096 edits. The offer copies them into the same
/// journal batch as the edits, which holds at most 8 MiB.
#[test]
fn four_thousand_pyrodigal_genes_fit_one_proposal_batch() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let (mut session, doc) = genome_session(temp.path());
    let actor = annotation_plugin();
    let ops: Vec<_> = (0..4096).map(pyrodigal_gene).collect();
    let result = session
        .apply_sequence_proposals(doc, offer(&session, doc, &ops, &actor))
        .unwrap();
    assert_eq!(result.applied.len(), 4096);
    let bytes = journal_bytes(temp.path());
    eprintln!("4096 Pyrodigal genes: {bytes} journal bytes of {}", 8 << 20);
    assert!(bytes < 6 << 20, "{bytes}");
}

#[test]
fn four_thousand_translated_genes_are_too_large_and_recorded_as_rejected() {
    use basecraft_protocol::ops::ProposalDecision;
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let (mut session, doc) = genome_session(temp.path());
    let actor = annotation_plugin();
    let ops: Vec<_> = (0..4096).map(translated_gene).collect();
    let error = session
        .apply_sequence_proposals(doc, offer(&session, doc, &ops, &actor))
        .unwrap_err();
    assert!(
        matches!(error, basecraft_ops::Error::Limit { .. }),
        "{error}"
    );
    eprintln!(
        "4096 translated genes, offer alone: {} journal bytes",
        journal_bytes(temp.path())
    );
    assert_eq!(session.doc(doc).unwrap().features().count(), 0);
    assert!(!session.can_undo(doc).unwrap());
    let entries = proposal_entries(temp.path());
    let names: Vec<_> = entries.iter().map(|e| e.op.name()).collect();
    assert_eq!(names, ["proposal.offer", "proposal.decide"]);
    let Op::ProposalDecide { outcome, note, .. } = &entries[1].op else {
        unreachable!()
    };
    assert_eq!(*outcome, ProposalDecision::Rejected);
    assert!(
        note.as_deref()
            .is_some_and(|note| note.starts_with("too large to apply")),
        "{note:?}"
    );
    assert!(records::replay(temp.path()).unwrap().ok());
}
