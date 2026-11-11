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
    if !name.starts_with("sanger_review") {
        assert!(!session.can_undo(doc));
    }
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
    assert_checked_access(&session, doc, name);
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

    // Old journals have only the legacy counts. Re-encode this real workflow
    // through the actual checksummed writer, rather than bypassing integrity.
    assert!(entries.iter().all(|entry| entry.state.counts.is_some()));
    let mut legacy = entries.clone();
    for entry in &mut legacy {
        entry.state.counts = None;
    }
    replace_fixture_log(dir.path(), &legacy);
    let legacy_report = replay::replay(dir.path()).unwrap();
    assert!(
        legacy_report.ok(),
        "legacy {name}: {:#?}",
        legacy_report.problems
    );
    assert_eq!(legacy_report.entries, entries.len());

    // Equal legacy counts do not make different document kinds equivalent.
    let mut wrong_kind = entries;
    let state = &mut wrong_kind[1].state;
    use basecraft_protocol::ops::DocumentCounts;
    state.counts = Some(
        if matches!(state.counts, Some(DocumentCounts::Sequence { .. })) {
            DocumentCounts::Tree {
                tips: state.len,
                annotations: state.features,
            }
        } else {
            DocumentCounts::Sequence {
                residues: state.len,
                features: state.features,
            }
        },
    );
    replace_fixture_log(dir.path(), &wrong_kind);
    assert!(
        !replay::replay(dir.path()).unwrap().ok(),
        "{name}: wrong kind accepted"
    );
}

/// Only the disposable scenario's journal is replaced; original inputs and
/// retained objects stay available to the real replay owner.
fn replace_fixture_log(root: &Path, entries: &[basecraft_protocol::LogEntry]) {
    std::fs::remove_dir_all(log::log_dir(root)).unwrap();
    log::LogWriter::new(root, "fingerprint-compatibility")
        .append_batch(entries)
        .unwrap();
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
        let a = s.alignment(d).unwrap();
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
        let tree = s.tree(d).unwrap();
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
            assert!(s.tree(d).unwrap().find_tip("specimen_01").is_some());
            assert!(s.tree(d).unwrap().find_tip("specimen_02").is_some());
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
        s.delimitation(d).unwrap().clone()
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
        s.metadata(d).unwrap().clone()
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
        s.submission(d).unwrap().clone()
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

// Exercise every public kind getter against each populated kind, plus missing
// IDs. These reads must preserve unsaved edits, history, identity and all
// project/recovery bytes, including the existing journal.
fn assert_checked_access(session: &Session, doc: DocId, name: &str) {
    use basecraft_ops::{Error, files::Kind};
    fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut out = BTreeMap::new();
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                out.extend(files(&path));
            } else {
                out.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
        out
    }
    let before_files = files(session.root());
    let before_state = session.state(doc);
    let before_revision = session.revision(doc);
    let before_key = session.doc_key(doc).to_owned();
    let before_undo = (session.can_undo(doc), session.can_redo(doc));
    for id in [doc, DocId(usize::MAX), DocId(doc.0 + 1)] {
        let checks = [
            (
                Kind::SangerReview,
                "sanger_review",
                session.sanger_review(id).map(|_| ()),
            ),
            (Kind::Sequence, "sequence", session.doc(id).map(|_| ())),
            (
                Kind::Sequence,
                "sequence",
                session.checked_sequence(id).map(|_| ()),
            ),
            (
                Kind::Alignment,
                "alignment",
                session.alignment(id).map(|_| ()),
            ),
            (Kind::Tree, "tree", session.tree(id).map(|_| ())),
            (
                Kind::Tree,
                "tree",
                session.tree_metadata_warning(id).map(|_| ()),
            ),
            (
                Kind::Delimitation,
                "delimitation",
                session.delimitation(id).map(|_| ()),
            ),
            (Kind::Metadata, "metadata", session.metadata(id).map(|_| ())),
            (
                Kind::Metadata,
                "metadata",
                session.metadata_notices(id).map(|_| ()),
            ),
            (
                Kind::Submission,
                "submission",
                session.submission(id).map(|_| ()),
            ),
        ];
        for (kind, expected, result) in checks {
            if id != doc {
                let error = result.unwrap_err();
                assert_eq!(error.code(), "unknown_document");
                assert!(matches!(error, Error::UnknownDocument { id: actual } if actual == id));
            } else if name == expected
                || (expected == "sanger_review" && name.starts_with("sanger_review"))
            {
                result.unwrap();
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.code(), "wrong_document_kind");
                assert!(
                    matches!(error, Error::WrongKind { id: actual, expected } if actual == id && expected == kind)
                );
            }
        }
        assert_eq!(session.is_alignment(id), id == doc && name == "alignment");
        assert_eq!(session.is_tree(id), id == doc && name == "tree");
        assert_eq!(
            session.is_delimitation(id),
            id == doc && name == "delimitation"
        );
        assert_eq!(session.is_metadata(id), id == doc && name == "metadata");
        assert_eq!(session.is_submission(id), id == doc && name == "submission");
    }
    assert_eq!(session.state(doc), before_state);
    assert_eq!(session.revision(doc), before_revision);
    assert_eq!(session.doc_key(doc), before_key);
    assert_eq!((session.can_undo(doc), session.can_redo(doc)), before_undo);
    assert_eq!(files(session.root()), before_files);
}

#[test]
fn getters_reject_an_empty_session() {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::unlogged(dir.path());
    let id = DocId(0);
    for result in [
        session.doc(id).map(|_| ()),
        session.checked_sequence(id).map(|_| ()),
        session.alignment(id).map(|_| ()),
        session.tree(id).map(|_| ()),
        session.tree_metadata_warning(id).map(|_| ()),
        session.delimitation(id).map(|_| ()),
        session.metadata(id).map(|_| ()),
        session.metadata_notices(id).map(|_| ()),
        session.submission(id).map(|_| ()),
    ] {
        assert_eq!(result.unwrap_err().code(), "unknown_document");
    }
    assert!(!session.is_alignment(id));
    assert!(!session.is_tree(id));
    assert!(!session.is_delimitation(id));
    assert!(!session.is_metadata(id));
    assert!(!session.is_submission(id));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn wrong_kind_extraction_does_not_begin_tracking_or_publish_output() {
    let dir = tempfile::tempdir().unwrap();
    let tree = fixture(dir.path(), "tree.nwk", b"(A:1,B:1);");
    let mut session = Session::new(dir.path());
    let doc = session.open(&tree).unwrap();
    let before = serde_json::to_value(log::read_all(dir.path()).unwrap()).unwrap();
    let output = dir.path().join("refused.gb");
    for (id, code) in [
        (doc, "wrong_document_kind"),
        (DocId(usize::MAX), "unknown_document"),
    ] {
        let error = session
            .extract(id, (0, 1), &[], false, &output)
            .unwrap_err();
        assert_eq!(error.code(), code);
        assert!(!output.exists());
        assert!(!session.can_undo(doc));
        assert_eq!(session.revision(doc), 0);
        assert_eq!(
            serde_json::to_value(log::read_all(dir.path()).unwrap()).unwrap(),
            before
        );
    }
    assert_eq!(std::fs::read(tree).unwrap(), b"(A:1,B:1);");
}

#[test]
fn sanger_review() {
    sanger_scenario("sanger_review", "single.json");
}

#[test]
fn sanger_review_multiple_call_sets() {
    sanger_scenario("sanger_review_multiple_call_sets", "multi.json");
}

fn sanger_scenario(name: &str, filename: &str) {
    use basecraft_protocol::sanger::{ReviewChoice, ReviewOp};
    let snapshot = |session: &Session, id| {
        let review = session.sanger_review(id).unwrap();
        (0..review.draft.columns.len())
            .map(|i| review.decision(i).unwrap().clone())
            .collect::<Vec<_>>()
    };
    scenario(
        name,
        |root| {
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/sanger-legacy")
                .join(filename);
            fixture(root, "review.json", &std::fs::read(source).unwrap())
        },
        snapshot,
        |session, id| {
            let original = snapshot(session, id);
            let mut first = original.clone();
            first[0] = ReviewChoice::Edit('N');
            let mut second = first.clone();
            second[1] = ReviewChoice::Edit('N');
            let call_set = session.sanger_review(id).unwrap().draft_sha256.clone();
            (
                [
                    Op::SangerReview {
                        call_set: call_set.clone(),
                        edit: Box::new(ReviewOp::Set {
                            columns: vec![0],
                            choice: first[0].clone(),
                        }),
                    },
                    Op::SangerReview {
                        call_set,
                        edit: Box::new(ReviewOp::Set {
                            columns: vec![1],
                            choice: second[1].clone(),
                        }),
                    },
                ],
                [original, first, second],
            )
        },
    );
}
