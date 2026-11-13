//! A reroot that leaves clade labels on other groups of tips reports each
//! one, and `tree.place_label` puts them where the user chooses (D11).
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{DocId, Session, records};
use basecraft_protocol::tree::{DisplacedLabel, LabelPlacement};
use basecraft_protocol::{CladeAnnotation, Op};

fn place(labels: &[u32], choice: LabelPlacement) -> Op {
    Op::TreePlaceLabel {
        labels: labels.to_vec(),
        choice,
    }
}

fn displaced(session: &Session, tree: DocId) -> Vec<(u32, bool)> {
    let labels = session.tree(tree).unwrap().displaced_labels();
    labels.iter().map(|l| (l.id, l.exact)).collect()
}

/// A rooted tree labelled on the whole tree ("All") and on a and b ("AB"),
/// then rooted on b: "All" now names a, c and d, which a clade still
/// covers exactly (the root); "AB" names the whole tree, and no clade holds
/// exactly a and b.
fn rerooted(root: &std::path::Path) -> (Session, DocId, Vec<DisplacedLabel>) {
    std::fs::write(root.join("t.nwk"), "[&R] ((a,b),(c,d));\n").unwrap();
    let mut session = Session::new(root);
    let tree = session.open(&root.join("t.nwk")).unwrap();
    for (id, clade, label) in [(1, ["a", "d"], "All"), (2, ["a", "b"], "AB")] {
        let annotation = Some(CladeAnnotation::new(clade.map(String::from).into(), label));
        session
            .apply(tree, Op::TreeAnnotate { id, annotation })
            .unwrap();
    }
    let reroot = Op::TreeReroot {
        clade: vec!["b".into()],
    };
    let displaced = session.apply(tree, reroot).unwrap().displaced;
    (session, tree, displaced)
}

#[test]
fn a_reroot_reports_each_displaced_label_and_whether_an_exact_clade_exists() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (session, tree, reported) = rerooted(dir.path());
    let report: Vec<_> = reported
        .iter()
        .map(|l| (l.id, l.label.as_str(), l.exact))
        .collect();
    assert_eq!(report, [(1, "All", true), (2, "AB", false)]);
    assert_eq!(displaced(&session, tree), [(1, true), (2, false)]);
}

#[test]
fn labels_are_placed_one_undo_step_at_a_time_and_replay() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, tree, _) = rerooted(dir.path());
    // No clade holds exactly a and b, so a request covering AB changes nothing.
    assert!(
        session
            .apply(tree, place(&[2], LabelPlacement::Exact))
            .is_err()
    );
    assert!(
        session
            .apply(tree, place(&[], LabelPlacement::Exact))
            .is_err()
    );
    assert_eq!(displaced(&session, tree), [(1, true), (2, false)]);

    session
        .apply(tree, place(&[1], LabelPlacement::Exact))
        .unwrap();
    assert_eq!(displaced(&session, tree), [(2, false)]);
    assert!(
        session
            .apply(tree, place(&[1], LabelPlacement::Remove))
            .is_err()
    );
    let outcome = session
        .apply(tree, place(&[2], LabelPlacement::Smallest))
        .unwrap();
    assert_eq!(
        outcome.notes,
        ["Label \"AB\" moved to the smallest clade holding its tips"]
    );
    assert_eq!(displaced(&session, tree), []);
    let annotations = session.tree(tree).unwrap().annotations();
    assert_eq!(
        annotations[&2].tips.len(),
        4,
        "AB now marks the clade it sits on"
    );

    session.undo(tree).unwrap();
    assert_eq!(displaced(&session, tree), [(2, false)]);
    session.undo(tree).unwrap();
    assert_eq!(displaced(&session, tree), [(1, true), (2, false)]);
    session.redo(tree).unwrap();
    session.redo(tree).unwrap();
    assert_eq!(displaced(&session, tree), []);

    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.unchecked, 0);

    // The side file keeps the placed labels' tips: nothing is displaced after
    // a reopen.
    session.save(tree, None).unwrap();
    drop(session);
    let mut reopened = Session::new(dir.path());
    let tree = reopened.open(&dir.path().join("t.nwk")).unwrap();
    assert_eq!(displaced(&reopened, tree), []);
    assert_eq!(reopened.tree(tree).unwrap().annotations()[&2].tips.len(), 4);
}

#[test]
fn a_reroot_keeps_labels_made_unrooted_on_their_surviving_split() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("t.nwk"), "((a,b),(c,d));\n").unwrap();
    let mut session = Session::new(dir.path());
    let tree = session.open(&dir.path().join("t.nwk")).unwrap();
    let annotation = Some(CladeAnnotation::new(vec!["a".into(), "b".into()], "AB"));
    session
        .apply(tree, Op::TreeAnnotate { id: 1, annotation })
        .unwrap();
    let reroot = Op::TreeReroot {
        clade: vec!["b".into()],
    };
    // a and b now sit on either side of the root, but the split ab|cd
    // survives as the clade c, d: the label moves there and is not displaced.
    assert_eq!(session.apply(tree, reroot).unwrap().displaced, []);
    let doc = session.tree(tree).unwrap();
    let label = &doc.annotations()[&1];
    let marked = doc.clade_tips(&label.clade).unwrap();
    assert_eq!(marked.iter().collect::<Vec<_>>(), ["c", "d"]);
    assert_eq!(label.tips.iter().collect::<Vec<_>>(), ["a", "b"]);
}

#[test]
fn removing_every_displaced_label_is_one_step_named_in_history() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, tree, _) = rerooted(dir.path());
    let outcome = session
        .apply(tree, place(&[2, 1, 2], LabelPlacement::Remove))
        .unwrap();
    assert_eq!(
        outcome.notes,
        ["Label \"All\" removed", "Label \"AB\" removed"]
    );
    assert!(session.tree(tree).unwrap().annotations().is_empty());
    // The log names each label once.
    let logged = basecraft_ops::log::read_all(dir.path()).unwrap();
    assert!(
        logged
            .iter()
            .any(|e| matches!(&e.op, Op::TreePlaceLabel { labels, .. } if labels == &[1, 2]))
    );
    let history =
        basecraft_ops::log::activity_text(&[dir.path().to_path_buf()], "1970-01-01T00:00:00Z", 0)
            .unwrap();
    assert!(history.contains("Label \"AB\" removed"), "{history}");
    session.undo(tree).unwrap();
    assert_eq!(displaced(&session, tree), [(1, true), (2, false)]);
    session.save(tree, None).unwrap();
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}
