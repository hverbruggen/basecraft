//! The common lifecycle contract for the seven Session kinds.
//! Keep semantic assertions as well as fingerprints: equal counts do not imply
//! equal documents.
use crate::sanger_fixture;

use std::{
    collections::BTreeMap,
    fmt::Debug,
    path::{Path, PathBuf},
    process::Command,
};

use basecraft_ops::{DocId, Session, log, metadata, records, submission};
use basecraft_protocol::delimitation::DelimitationEdit;
use basecraft_protocol::metadata::{ChangeSet, MetadataEdit, Row};
use basecraft_protocol::ops::DocumentCounts;
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
            .args([
                "--exact",
                &crate::isolated_profile::test_name(),
                "--nocapture",
            ])
            .env("BASECRAFT_KIND_CONFORMANCE_CHILD", name)
            .env("BASECRAFT_PROFILE_DIR", isolated.path().join("profile"))
            .env("BASECRAFT_STATE_DIR", isolated.path().join("state"))
            .env("BASECRAFT_CACHE_DIR", isolated.path().join("cache"))
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
    // The save replaces the original; replay checks the edits only once it is back.
    let mut originals = Vec::new();
    let mut pending = vec![dir.path().to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                originals.push((path, bytes));
            }
        }
    }
    let actor = Actor::Script {
        client: "kind-conformance".into(),
    };
    let mut session = Session::new(dir.path());
    session.set_actor(actor.clone());
    let (doc, kind) = session.open_classified(&path).unwrap();
    let (ops, expected) = plan(&session, doc);
    assert_eq!(snapshot(&session, doc), expected[0]);
    assert!(!session.can_undo(doc).unwrap());
    assert!(!session.can_redo(doc).unwrap());
    let first = session.apply(doc, ops[0].clone()).unwrap();
    assert_eq!(snapshot(&session, doc), expected[1]);
    assert_ne!(expected[0], expected[1]);
    // A command group keeps the second edit its own undo step even when it
    // would continue a typing burst.
    session.begin_group();
    let second = session.apply(doc, ops[1].clone()).unwrap();
    session.end_group();
    assert_eq!(snapshot(&session, doc), expected[2]);
    assert_ne!(expected[1], expected[2]);
    assert_ne!(first.entry_id, second.entry_id);
    assert!(session.can_undo(doc).unwrap());
    assert!(!session.can_redo(doc).unwrap());
    assert_eq!(session.undo(doc).unwrap(), second.entry_id);
    assert_eq!(snapshot(&session, doc), expected[1]);
    assert!(session.can_undo(doc).unwrap());
    assert!(session.can_redo(doc).unwrap());
    assert_eq!(session.redo(doc).unwrap(), second.entry_id);
    assert_eq!(snapshot(&session, doc), expected[2]);
    assert!(!session.can_redo(doc).unwrap());
    assert_checked_access(&session, doc, kind);
    let fingerprint = session.state(doc).unwrap();
    session.save(doc, None).unwrap();
    assert_eq!(snapshot(&session, doc), expected[2]);
    drop(session);

    let entries = log::read_all(dir.path()).unwrap();
    // D22: an alphabet answer is in the side file at once, so its save has
    // nothing to write and logs nothing.
    let saved = !matches!(ops[0], Op::SetAlphabet { .. });
    assert_eq!(
        entries.len(),
        5 + usize::from(saved),
        "open, two edits, undo, redo, save"
    );
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
    if saved {
        assert!(matches!(entries[5].op, Op::Save { .. }));
        assert_ne!(entries[5].result.hashes, Vec::new());
    }
    assert!(
        entries
            .iter()
            .all(|e| e.actor == actor && e.doc == entries[0].doc)
    );

    let mut reopened = Session::unlogged(dir.path());
    let copy = reopened.open(&path).unwrap();
    assert_eq!(snapshot(&reopened, copy), expected[2]);
    assert_eq!(reopened.state(copy).unwrap(), fingerprint);
    for (path, bytes) in &originals {
        std::fs::write(path, bytes).unwrap();
    }
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.unchecked, 0);
    assert_eq!(report.sessions, 1);
    assert_eq!(report.entries, entries.len());
    assert_eq!(log::read_all(dir.path()).unwrap().len(), entries.len());

    // Equal aggregate counts do not make different document kinds equivalent.
    let mut wrong_kind = entries;
    let state = &mut wrong_kind[1].state;
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
                records: None,
            }
        },
    );
    replace_fixture_log(dir.path(), &wrong_kind);
    assert!(
        !records::replay(dir.path()).unwrap().ok(),
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
    crate::isolated_profile::isolate!();
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
                    "../../../../testdata/fixtures/compact/sequences-annotations/locus.gff3"
                ),
            );
            fixture(
                root,
                "locus.fasta",
                include_bytes!(
                    "../../../../testdata/fixtures/compact/sequences-annotations/locus.fasta"
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
                Op::seq_replace(0, 3, "NNN"),
                Op::feature_update(second.1[0].0.0, second.1[0].1.clone()),
            ];
            (ops, [original, first, second])
        },
    );
}

#[test]
fn alignment() {
    crate::isolated_profile::isolate!();
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
                    "../../../../testdata/fixtures/compact/alignment-editing/homologues.afa"
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
    crate::isolated_profile::isolate!();
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
                    "../../../../testdata/fixtures/compact/species-delimitation/time-binary.nwk"
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
            // The tree records the tips each label was made for.
            let made = |label: &CladeAnnotation| CladeAnnotation {
                tips: label.clade.iter().cloned().collect(),
                ..label.clone()
            };
            let mut first = original.clone();
            first.2.insert(1, made(&a));
            let mut second = first.clone();
            second.2.insert(2, made(&b));
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
    crate::isolated_profile::isolate!();
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
                    "../../../../testdata/fixtures/compact/species-delimitation/hypotheses.spart"
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
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
                    "../../../../testdata/fixtures/compact/alignment-editing/homologues.afa"
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
fn assert_checked_access(session: &Session, doc: DocId, actual_kind: basecraft_ops::files::Kind) {
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
    let before_state = session.state(doc).unwrap();
    let before_revision = session.revision(doc).unwrap();
    let before_key = session.doc_key(doc).unwrap().to_owned();
    let before_undo = (
        session.can_undo(doc).unwrap(),
        session.can_redo(doc).unwrap(),
    );
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
        for (kind, _expected, result) in checks {
            if id != doc {
                let error = result.unwrap_err();
                assert_eq!(error.code(), "unknown_document");
                assert!(matches!(error, Error::UnknownDocument { id: actual } if actual == id));
            } else if actual_kind == kind {
                result.unwrap();
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.code(), "wrong_document_kind");
                assert!(
                    matches!(error, Error::WrongKind { id: actual, expected } if actual == id && expected == kind)
                );
            }
        }
        assert_eq!(
            session.is_alignment(id),
            id == doc && actual_kind == Kind::Alignment
        );
        assert_eq!(session.is_tree(id), id == doc && actual_kind == Kind::Tree);
        assert_eq!(
            session.is_delimitation(id),
            id == doc && actual_kind == Kind::Delimitation
        );
        assert_eq!(
            session.is_metadata(id),
            id == doc && actual_kind == Kind::Metadata
        );
        assert_eq!(
            session.is_submission(id),
            id == doc && actual_kind == Kind::Submission
        );
    }
    assert_eq!(session.state(doc).unwrap(), before_state);
    assert_eq!(session.revision(doc).unwrap(), before_revision);
    assert_eq!(session.doc_key(doc).unwrap(), before_key);
    assert_eq!(
        (
            session.can_undo(doc).unwrap(),
            session.can_redo(doc).unwrap()
        ),
        before_undo
    );
    assert_eq!(files(session.root()), before_files);
}

#[test]
fn getters_reject_an_empty_session() {
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
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
            .extract(id, (0, 1), &[], false, &output, None)
            .unwrap_err();
        assert_eq!(error.code(), code);
        assert!(!output.exists());
        assert!(!session.can_undo(doc).unwrap());
        assert_eq!(session.revision(doc).unwrap(), 0);
        assert_eq!(
            serde_json::to_value(log::read_all(dir.path()).unwrap()).unwrap(),
            before
        );
    }
    assert_eq!(std::fs::read(tree).unwrap(), b"(A:1,B:1);");
}

#[test]
fn sanger_review() {
    crate::isolated_profile::isolate!();
    sanger_scenario("sanger_review", "single.json");
}

#[test]
fn sanger_review_multiple_call_sets() {
    crate::isolated_profile::isolate!();
    sanger_scenario("sanger_review_multiple_call_sets", "multi.json");
}

fn sanger_scenario(name: &str, filename: &str) {
    use basecraft_protocol::sanger::{ReviewChoice, ReviewOp};
    let snapshot = |session: &Session, id| -> serde_json::Value {
        serde_json::from_str(
            &session
                .sanger_review(id)
                .unwrap()
                .document_report(false)
                .unwrap()
                .raw,
        )
        .unwrap()
    };
    scenario(
        name,
        |root| sanger_fixture::copy_to(root, filename),
        snapshot,
        |session, id| {
            let original = snapshot(session, id);
            let mut first = original.clone();
            first["decisions"][0] = serde_json::to_value(ReviewChoice::Edit('N')).unwrap();
            first["manual"][0] = true.into();
            first["events"].as_array_mut().unwrap().push(
                serde_json::to_value(ReviewOp::Set {
                    columns: vec![0],
                    choice: ReviewChoice::Edit('N'),
                })
                .unwrap(),
            );
            let mut second = first.clone();
            second["decisions"][1] = serde_json::to_value(ReviewChoice::Edit('N')).unwrap();
            second["manual"][1] = true.into();
            second["events"].as_array_mut().unwrap().push(
                serde_json::to_value(ReviewOp::Set {
                    columns: vec![1],
                    choice: ReviewChoice::Edit('N'),
                })
                .unwrap(),
            );
            let call_set = session.sanger_review(id).unwrap().draft_sha256.clone();
            (
                [
                    Op::SangerReview {
                        call_set: call_set.clone(),
                        edit: Box::new(ReviewOp::Set {
                            columns: vec![0],
                            choice: ReviewChoice::Edit('N'),
                        }),
                    },
                    Op::SangerReview {
                        call_set,
                        edit: Box::new(ReviewOp::Set {
                            columns: vec![1],
                            choice: ReviewChoice::Edit('N'),
                        }),
                    },
                ],
                [original, first, second],
            )
        },
    );
}

/// A stated alphabet lives only in the alignment's companion; it survives
/// undo, redo, save, reopen and replay like any edit.
#[test]
fn alignment_alphabet() {
    use basecraft_protocol::facts::{Alphabet, Fact, FactSource, StatedAlphabet};
    crate::isolated_profile::isolate!();
    let stated = |value| Fact {
        value,
        source: FactSource::User,
    };
    scenario(
        "alignment_alphabet",
        |root| fixture(root, "pair.afa", b">a\nACGT\n>b\nACGA\n"),
        |s, d| s.alphabet(d).unwrap(),
        |s, d| {
            let original = s.alphabet(d).unwrap();
            assert_eq!(original.source, FactSource::Guessed);
            (
                [StatedAlphabet::Rna, StatedAlphabet::Dna]
                    .map(|alphabet| Op::SetAlphabet { alphabet }),
                [original, stated(Alphabet::Rna), stated(Alphabet::Dna)],
            )
        },
    );
}

#[test]
fn tree_rooting() {
    use basecraft_protocol::facts::Rooting;
    crate::isolated_profile::isolate!();
    // Untagged with two clades at the base: a guessed root (D18), which the
    // user states unrooted and then rooted. The saved tag keeps the reading;
    // reopened, the file states it.
    scenario(
        "tree_rooting",
        |root| fixture(root, "t.nwk", b"((a:1,b:1):1,(c:1,d:1):1);\n"),
        |s, d| s.tree(d).unwrap().rooting().value,
        |_, _| {
            (
                [false, true].map(|rooted| Op::TreeSetRooting { rooted }),
                [Rooting::Rooted, Rooting::Unrooted, Rooting::Rooted],
            )
        },
    );
}

#[test]
fn tree_branch_lengths() {
    use basecraft_protocol::facts::{BranchLengths, Fact, FactSource};
    crate::isolated_profile::isolate!();
    let stated = |value| {
        Some(Fact {
            value,
            source: FactSource::User,
        })
    };
    scenario(
        "tree_branch_lengths",
        |root| fixture(root, "t.nwk", b"((a:1,b:1):1,c:2);\n"),
        |s, d| s.tree(d).unwrap().branch_lengths().cloned(),
        |_, _| {
            let meanings = [
                BranchLengths::Substitutions,
                BranchLengths::AbsoluteTime { unit: "Ma".into() },
            ];
            (
                meanings
                    .clone()
                    .map(|meaning| Op::TreeSetBranchLengths { meaning }),
                [
                    None,
                    stated(meanings[0].clone()),
                    stated(meanings[1].clone()),
                ],
            )
        },
    );
}
