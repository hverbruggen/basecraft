//! A reroot that leaves clade labels on other groups of tips reports each
//! one, and `tree.place_label` puts them where the user chooses (D11).

use basecraft_ops::{DocId, Session, records};
use basecraft_protocol::tree::{DisplacedLabel, LabelPlacement};
use basecraft_protocol::{CladeAnnotation, Op};

fn place(labels: &[u32], choice: LabelPlacement) -> Op {
    Op::TreePlaceLabel {
        labels: labels.to_vec(),
        choice,
    }
}

fn displaced_now(session: &Session, tree: DocId) -> Vec<(u32, bool)> {
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
    assert_eq!(displaced_now(&session, tree), [(1, true), (2, false)]);
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
    assert_eq!(displaced_now(&session, tree), [(1, true), (2, false)]);

    session
        .apply(tree, place(&[1], LabelPlacement::Exact))
        .unwrap();
    assert_eq!(displaced_now(&session, tree), [(2, false)]);
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
    assert_eq!(displaced_now(&session, tree), []);
    let annotations = session.tree(tree).unwrap().annotations();
    assert_eq!(
        annotations[&2].tips.len(),
        4,
        "AB now marks the clade it sits on"
    );

    session.undo(tree).unwrap();
    assert_eq!(displaced_now(&session, tree), [(2, false)]);
    session.undo(tree).unwrap();
    assert_eq!(displaced_now(&session, tree), [(1, true), (2, false)]);
    session.redo(tree).unwrap();
    session.redo(tree).unwrap();
    assert_eq!(displaced_now(&session, tree), []);

    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
    assert_eq!(report.unchecked, 0);

    // The side file keeps the placed labels' tips: nothing is displaced after
    // a reopen.
    session.save(tree, None).unwrap();
    drop(session);
    let mut reopened = Session::new(dir.path());
    let tree = reopened.open(&dir.path().join("t.nwk")).unwrap();
    assert_eq!(displaced_now(&reopened, tree), []);
    assert_eq!(reopened.tree(tree).unwrap().annotations()[&2].tips.len(), 4);
}

/// A tree written as `text`, labelled "AB" on a and b, then rooted on b:
/// a and b now sit on either side of the root, while the split ab|cd
/// survives as the clade c, d.
fn rerooted_split(root: &std::path::Path, text: &str) -> (Session, DocId, Vec<DisplacedLabel>) {
    std::fs::write(root.join("t.nwk"), text).unwrap();
    let mut session = Session::new(root);
    let tree = session.open(&root.join("t.nwk")).unwrap();
    let annotation = Some(CladeAnnotation::new(vec!["a".into(), "b".into()], "AB"));
    session
        .apply(tree, Op::TreeAnnotate { id: 1, annotation })
        .unwrap();
    let reroot = Op::TreeReroot {
        clade: vec!["b".into()],
    };
    let displaced = session.apply(tree, reroot).unwrap().displaced;
    (session, tree, displaced)
}

/// The tips the label marks now, and the tips it was made for.
fn marked(session: &Session, tree: DocId) -> (Vec<String>, Vec<String>) {
    let doc = session.tree(tree).unwrap();
    let label = &doc.annotations()[&1];
    let now = doc.clade_tips(&label.clade).unwrap().into_iter().collect();
    (now, label.tips.iter().cloned().collect())
}

#[test]
fn on_a_tree_marked_unrooted_a_label_follows_its_surviving_split() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (session, tree, displaced) = rerooted_split(dir.path(), "[&U] ((a,b),(c,d));\n");
    assert_eq!(displaced, []);
    assert_eq!(
        marked(&session, tree),
        (vec!["c".into(), "d".into()], vec!["a".into(), "b".into()])
    );
}

#[test]
fn on_an_untagged_tree_the_base_decides_whether_a_label_marks_a_split() {
    crate::isolated_profile::isolate!();
    // Three clades at the base read as unrooted (D18): the label follows its
    // split, as on a tree marked unrooted.
    let dir = tempfile::tempdir().unwrap();
    let (session, tree, displaced) = rerooted_split(dir.path(), "((a,b),c,d);\n");
    assert_eq!(displaced, []);
    assert_eq!(
        marked(&session, tree),
        (vec!["c".into(), "d".into()], vec!["a".into(), "b".into()])
    );
    // Two clades read as rooted: the label marks a clade, which the reroot
    // leaves without an exact match, so the user is asked.
    let dir = tempfile::tempdir().unwrap();
    let (_, _, displaced) = rerooted_split(dir.path(), "((a,b),(c,d));\n");
    let report: Vec<_> = displaced.iter().map(|l| (l.id, l.exact)).collect();
    assert_eq!(report, [(1, false)]);
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
    assert_eq!(displaced_now(&session, tree), [(1, true), (2, false)]);
    session.save(tree, None).unwrap();
    let report = records::replay(dir.path()).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}
