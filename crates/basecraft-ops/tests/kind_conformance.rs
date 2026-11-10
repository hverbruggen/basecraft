//! The common lifecycle contract for the six existing Session kinds.
//! Keep semantic assertions as well as fingerprints: equal counts do not imply
//! equal documents. Sanger review is not a Session kind yet.
use std::{
    collections::BTreeMap,
    fmt::Debug,
    path::{Path, PathBuf},
    process::Command,
};

use basecraft_ops::{DocId, Session, log, metadata, replay, submission};
use basecraft_protocol::delimitation::DelimitationEdit;
use basecraft_protocol::metadata::{ChangeSet, MetadataEdit, Row};
use basecraft_protocol::submission::{Decision, Publication, RowDecision, SubmissionEdit};
use basecraft_protocol::{Actor, CladeAnnotation, Op};

// GFF3 stores source/phase in columns and other qualifiers in attributes.
// Reading a joined feature can change their order, but not their values or
// multiplicity. Compare that semantic multiset without dropping any qualifier.
fn feature_semantics(mut feature: basecraft_protocol::Feature) -> basecraft_protocol::Feature {
    feature
        .qualifiers
        .sort_by(|a, b| (&a.key, &a.value).cmp(&(&b.key, &b.value)));
    feature
}

fn fixture(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Each scenario has a separate process/profile as well as a project directory:
/// save guards discover application state independently of the Session root.
fn scenario<T: Debug + PartialEq>(
    name: &str,
    prepare: impl FnOnce(&Path) -> PathBuf,
    snapshot: impl Fn(&Session, DocId) -> T,
    plan: impl FnOnce(&Session, DocId) -> ([Op; 2], [T; 3]),
) {
    if std::env::var("BASECRAFT_KIND_CONFORMANCE_CHILD").as_deref() != Ok(name) {
        let isolated = tempfile::tempdir().unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("BASECRAFT_KIND_CONFORMANCE_CHILD", name)
            .env("BASECRAFT_PROFILE_DIR", isolated.path().join("profile"))
            .env("BASECRAFT_STATE_DIR", isolated.path().join("state"))
            .env("BASECRAFT_CACHE_DIR", isolated.path().join("cache"))
            .env("BASECRAFT_SCRATCH_DIR", isolated.path().join("scratch"))
            .env("TMPDIR", isolated.path())
            .env("TMP", isolated.path())
            .env("TEMP", isolated.path())
            .status()
            .unwrap();
        assert!(status.success(), "{name}: isolated scenario failed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = prepare(dir.path());
    let actor = Actor::Script {
        client: "kind-conformance".into(),
    };
    let mut session = Session::new(dir.path());
    session.set_actor(actor.clone());
    let doc = session.open(&path).unwrap();
    let (ops, expected) = plan(&session, doc);
    assert_eq!(snapshot(&session, doc), expected[0]);
    assert!(!session.can_undo(doc));
    assert!(!session.can_redo(doc));
    let first = session.apply(doc, ops[0].clone()).unwrap();
    session.flush().unwrap(); // Distinct undo steps even for mergeable typing.
    assert_eq!(snapshot(&session, doc), expected[1]);
    assert_ne!(expected[0], expected[1]);
    let second = session.apply(doc, ops[1].clone()).unwrap();
    session.flush().unwrap();
    assert_eq!(snapshot(&session, doc), expected[2]);
    assert_ne!(expected[1], expected[2]);
    assert_ne!(first.entry_id, second.entry_id);
    assert!(session.can_undo(doc));
    assert!(!session.can_redo(doc));
    assert_eq!(session.undo(doc).unwrap(), second.entry_id);
    assert_eq!(snapshot(&session, doc), expected[1]);
    assert!(session.can_undo(doc));
    assert!(session.can_redo(doc));
    assert_eq!(session.redo(doc).unwrap(), second.entry_id);
    assert_eq!(snapshot(&session, doc), expected[2]);
    assert!(!session.can_redo(doc));
    let fingerprint = session.state(doc);
    session.save(doc, None).unwrap();
    session.flush().unwrap();
    assert_eq!(snapshot(&session, doc), expected[2]);
    drop(session);

    let entries = log::read_all(dir.path()).unwrap();
    assert_eq!(entries.len(), 6, "open, two edits, undo, redo, save");
    assert!(matches!(entries[0].op, Op::Open { .. }));
    assert_eq!(entries[1].op, ops[0]);
    assert_eq!(entries[2].op, ops[1]);
    assert_eq!(entries[1].id, first.entry_id);
    assert_eq!(entries[2].id, second.entry_id);
    assert_eq!(
        entries[3].op,
        Op::Undo {
            of: second.entry_id.clone()
        }
    );
    assert_eq!(
        entries[4].op,
        Op::Redo {
            of: second.entry_id
        }
    );
    assert!(matches!(entries[5].op, Op::Save { .. }));
    assert_ne!(entries[5].result.hashes, Vec::new());
    assert!(
        entries
            .iter()
            .all(|e| e.actor == actor && e.doc == entries[0].doc)
    );

    let mut reopened = Session::unlogged(dir.path());
    let copy = reopened.open(&path).unwrap();
    assert_eq!(snapshot(&reopened, copy), expected[2]);
    assert_eq!(reopened.state(copy), fingerprint);
    let report = replay::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.sessions, 1);
    assert_eq!(report.entries, entries.len());
    assert_eq!(log::read_all(dir.path()).unwrap().len(), entries.len());
}

#[test]
fn sequence() {
    let snapshot = |s: &Session, d| {
        let seq = s.checked_sequence(d).unwrap();
        (
            seq.read(0, seq.len()),
            seq.features()
                .map(|(id, f)| (id, feature_semantics(f.clone())))
                .collect::<Vec<_>>(),
        )
    };
    scenario(
        "sequence",
        |root| {
            fixture(
                root,
                "locus.gff3",
                include_bytes!(
                    "../../../testdata/fixtures/compact/sequences-annotations/locus.gff3"
                ),
            );
            fixture(
                root,
                "locus.fasta",
                include_bytes!(
                    "../../../testdata/fixtures/compact/sequences-annotations/locus.fasta"
                ),
            )
        },
        snapshot,
        |s, d| {
            let original = snapshot(s, d);
            assert_ne!(original.1, Vec::new());
            let mut first = original.clone();
            first.0[0..3].copy_from_slice(b"NNN");
            let mut second = first.clone();
            second.1[0]
                .1
                .qualifiers
                .push(basecraft_protocol::Qualifier::new("note", "reviewed"));
            second.1[0].1 = feature_semantics(second.1[0].1.clone());
            let ops = [
                Op::SeqReplace {
                    start: 0,
                    end: 3,
                    residues: "NNN".into(),
                },
                Op::FeatureUpdate {
                    id: second.1[0].0.0,
                    feature: second.1[0].1.clone(),
                },
            ];
            (ops, [original, first, second])
        },
    );
}

#[test]
fn alignment() {
    let snapshot = |s: &Session, d| {
        assert!(s.is_alignment(d));
        let a = s.alignment(d);
        (0..a.row_count())
            .map(|row| (a.row(row).name.clone(), a.read(row, 0, a.width())))
            .collect::<Vec<_>>()
    };
    scenario(
        "alignment",
        |root| {
            fixture(
                root,
                "homologues.afa",
                include_bytes!(
                    "../../../testdata/fixtures/compact/alignment-editing/homologues.afa"
                ),
            )
        },
        snapshot,
        |s, d| {
            let original = snapshot(s, d);
            assert_eq!(original.len(), 8);
            let width = original[0].1.len();
            let mut first = original.clone();
            first.push(("added_a".into(), vec![b'A'; width]));
            let mut second = first.clone();
            second.push(("added_c".into(), vec![b'C'; width]));
            // Existing named-row addition avoids introducing another index-addressed
            // edit while the production stable-row-identity migration is pending.
            let ops =
                [("added_a", "A"), ("added_c", "C")].map(|(name, residue)| Op::AlnAddSequences {
                    rows: vec![(name.into(), residue.repeat(width))],
                    reference: None,
                });
            (ops, [original, first, second])
        },
    );
}

#[test]
fn tree() {
    let snapshot = |s: &Session, d| {
        assert!(s.is_tree(d));
        let tree = s.tree(d);
        (
            tree.to_newick(),
            tree.supports().to_vec(),
            tree.annotations().clone(),
        )
    };
    scenario(
        "tree",
        |root| {
            fixture(
                root,
                "tree.nwk",
                include_bytes!(
                    "../../../testdata/fixtures/compact/species-delimitation/time-binary.nwk"
                ),
            )
        },
        snapshot,
        |s, d| {
            let original = snapshot(s, d);
            assert!(s.tree(d).find_tip("specimen_01").is_some());
            assert!(s.tree(d).find_tip("specimen_02").is_some());
            let a = CladeAnnotation::new(vec!["specimen_01".into()], "Reviewed tip");
            let b = CladeAnnotation::new(vec!["specimen_02".into()], "Second tip");
            let mut first = original.clone();
            first.2.insert(1, a.clone());
            let mut second = first.clone();
            second.2.insert(2, b.clone());
            (
                [
                    Op::TreeAnnotate {
                        id: 1,
                        annotation: Some(a),
                    },
                    Op::TreeAnnotate {
                        id: 2,
                        annotation: Some(b),
                    },
                ],
                [original, first, second],
            )
        },
    );
}

#[test]
fn delimitation() {
    let snapshot = |s: &Session, d| {
        assert!(s.is_delimitation(d));
        s.delimitation(d).clone()
    };
    scenario(
        "delimitation",
        |root| {
            fixture(
                root,
                "hypotheses.spart",
                include_bytes!(
                    "../../../testdata/fixtures/compact/species-delimitation/hypotheses.spart"
                ),
            )
        },
        snapshot,
        |s, d| {
            let original = snapshot(s, d);
            assert_eq!(original.hypotheses.len(), 2);
            let mut first = original.clone();
            first.hypotheses[0].name = "Reviewed construction".into();
            let mut second = first.clone();
            second.hypotheses[1].name = "Reviewed partial".into();
            let ops = [0, 1].map(|i| Op::DelimitationEdit {
                edit: Box::new(DelimitationEdit::Rename {
                    hypothesis: original.hypotheses[i].id.clone(),
                    name: second.hypotheses[i].name.clone(),
                }),
            });
            (ops, [original, first, second])
        },
    );
}

#[test]
fn metadata() {
    let snapshot = |s: &Session, d| {
        assert!(s.is_metadata(d));
        s.metadata(d).clone()
    };
    scenario(
        "metadata",
        |root| metadata::create(root).unwrap(),
        snapshot,
        |s, d| {
            let original = snapshot(s, d);
            let mut first = original.clone();
            first.specimens.rows.push(Row {
                id: "specimen-a".into(),
                cells: BTreeMap::new(),
            });
            let mut second = first.clone();
            second.specimens.rows.push(Row {
                id: "specimen-b".into(),
                cells: BTreeMap::new(),
            });
            let ops = ["specimen-a", "specimen-b"].map(|id| Op::MetadataEdit {
                edit: Box::new(MetadataEdit::Apply {
                    changes: ChangeSet {
                        specimens: vec![id.into()],
                        ..ChangeSet::default()
                    },
                }),
            });
            (ops, [original, first, second])
        },
    );
}

#[test]
fn submission() {
    let snapshot = |s: &Session, d| {
        assert!(s.is_submission(d));
        s.submission(d).clone()
    };
    scenario(
        "submission",
        |root| {
            let alignment = fixture(
                root,
                "source.afa",
                include_bytes!(
                    "../../../testdata/fixtures/compact/alignment-editing/homologues.afa"
                ),
            );
            let path = root.join("review.submission.json");
            submission::create(&mut Session::unlogged(root), &path, &alignment).unwrap();
            path
        },
        snapshot,
        |s, d| {
            let original = snapshot(s, d);
            assert!(original.source.is_some());
            let mut first = original.clone();
            first.publication = Publication {
                title: "Reviewed marker sequences".into(),
                authors: vec![],
            };
            let mut second = first.clone();
            second.rows = vec![RowDecision {
                record: "specimen_01".into(),
                decision: Decision::Exclude,
            }];
            (
                [
                    Op::SubmissionEdit {
                        edit: Box::new(SubmissionEdit::SetPublication {
                            publication: first.publication.clone(),
                        }),
                    },
                    Op::SubmissionEdit {
                        edit: Box::new(SubmissionEdit::SetRows {
                            rows: second.rows.clone(),
                        }),
                    },
                ],
                [original, first, second],
            )
        },
    );
}
