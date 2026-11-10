//! Fault injection at the real QObject boundary, run by application_bootstrap.
use super::*;

fn state(controller: &qobject::TreeController, original: DocId) -> Value {
    let r = controller.rust();
    let session = r.session.as_ref().unwrap();
    json!({
        "doc": r.doc.map(|id| id.0), "path": r.file_path.to_string(),
        "session": session.id(), "key": session.doc_key(original),
        "revision": session.revision(original),
        "undo": session.can_undo(original), "redo": session.can_redo(original),
        "record": session.document_record(original).unwrap(),
        "selected": r.view.selected, "multi": r.view.multi_taxa,
        "find": crate::find_bridge::status(&r.find, 0.0),
        "find_revision": r.find.revision(), "find_status_revision": r.find_status_revision,
        "find_pending": r.find.busy(), "busy": r.busy, "dirty": r.dirty,
    })
}

pub(crate) fn run(mut controller: Pin<&mut qobject::TreeController>) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("tree.nwk");
    let sequence = folder.path().join("sequence.fa");
    std::fs::write(&path, "((A:1,B:2):3,(C:4,D:5):6);\n").unwrap();
    std::fs::write(&sequence, ">sequence\nACGT\n").unwrap();
    assert!(
        controller
            .as_mut()
            .open_file(&QString::from(path.to_str().unwrap()))
    );
    let original = controller.rust().doc.unwrap();
    let mut style = controller
        .rust()
        .session
        .as_ref()
        .unwrap()
        .tree(original)
        .unwrap()
        .style()
        .clone();
    style.tip_font += 1;
    let style = QString::from(serde_json::to_string(&style).unwrap());
    assert!(controller.as_mut().set_style(&style).is_empty());
    assert!(
        controller
            .rust()
            .session
            .as_ref()
            .unwrap()
            .can_undo(original)
    );
    let wrong = controller
        .as_mut()
        .rust_mut()
        .session
        .as_mut()
        .unwrap()
        .open(&sequence)
        .unwrap();
    controller.as_mut().rust_mut().view.selected = Some(1);
    // A pending query must not be replaced, advanced or polled on refusal.
    controller
        .as_mut()
        .rust_mut()
        .find
        .set_query("A".into(), false);
    for invalid in [wrong, DocId(usize::MAX)] {
        controller.as_mut().rust_mut().doc = Some(invalid);
        let before = state(&controller, original);
        let refusal = controller.as_mut().set_style(&style).to_string();
        assert!(
            !refusal.is_empty(),
            "style must not report success after refusal"
        );
        assert_eq!(controller.rust().message.to_string(), refusal);
        assert_eq!(controller.rust().message_kind.to_string(), "error");
        assert_eq!(state(&controller, original), before);
        controller.as_mut().ladderize(true);
        assert_eq!(controller.rust().message.to_string(), refusal);
        controller
            .as_mut()
            .find_query(&QString::from("replacement"), true);
        assert!(controller.as_mut().find_poll(1).is_empty());
        controller.as_mut().press(10.0, 10.0);
        assert_eq!(state(&controller, original), before);
    }
    controller.as_mut().rust_mut().doc = Some(original);
    assert!(controller.as_mut().set_style(&style).is_empty());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "((A:1,B:2):3,(C:4,D:5):6);\n"
    );
    assert_eq!(
        std::fs::read_to_string(sequence).unwrap(),
        ">sequence\nACGT\n"
    );
    println!("Tree owned-controller wrong-kind/missing-ID refusal passed");
}
