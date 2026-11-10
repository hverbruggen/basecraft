//! Fault injection at the real `QObject` boundary, run by `application_bootstrap`.
use super::*;

fn state(controller: &qobject::AlignmentController, original: DocId) -> serde_json::Value {
    let r = controller.rust();
    let session = r.session.as_ref().unwrap();
    json!({
        "doc": r.doc.map(|id| id.0), "path": r.file_path.to_string(),
        "session": session.id(), "key": session.doc_key(original),
        "revision": session.revision(original),
        "undo": session.can_undo(original), "redo": session.can_redo(original),
        "record": session.document_record(original).unwrap(),
        "view": format!("{:?}", r.view),
        "find": crate::find_bridge::status(&r.find, 0.0),
        "find_revision": r.find.revision(), "find_status_revision": r.find_status_revision,
        "find_pending": r.find.busy(), "busy": r.busy, "dirty": r.dirty,
        "tool_pending": r.tool_job.is_some(), "snapshot_pending": r.review_snapshot_dir.is_some(),
    })
}

pub(crate) fn run(mut controller: Pin<&mut qobject::AlignmentController>) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("protein.afa");
    let sequence = folder.path().join("sequence.fa");
    let content = ">A\nMELK\n>B\nMELR\n>C\nMELW\n>D\nMELF\n";
    std::fs::write(&path, content).unwrap();
    std::fs::write(&sequence, ">sequence\nACGT\n").unwrap();
    assert!(
        controller
            .as_mut()
            .open_file(&QString::from(path.to_str().unwrap()))
    );
    let original = controller.rust().doc.unwrap();
    assert!(
        controller
            .as_mut()
            .rename_row(&QString::from("renamed"))
            .is_empty()
    );
    assert!(
        controller
            .rust()
            .session
            .as_ref()
            .unwrap()
            .can_undo(original)
    );
    let plugin = QString::from("iqtree");
    let form = QString::from("tree");
    let valid = controller.tool_form(&plugin, &form).to_string();
    assert!(
        valid.contains("LG") && !valid.contains("GTR"),
        "protein-specific form: {valid}"
    );
    let wrong = controller
        .as_mut()
        .rust_mut()
        .session
        .as_mut()
        .unwrap()
        .open(&sequence)
        .unwrap();
    controller.as_mut().rust_mut().view.cursor = (1, 2);
    controller
        .as_mut()
        .rust_mut()
        .find
        .set_query("M".into(), false);
    for invalid in [wrong, DocId(usize::MAX)] {
        controller.as_mut().rust_mut().doc = Some(invalid);
        let before = state(&controller, original);
        let result: serde_json::Value =
            serde_json::from_str(&controller.tool_form(&plugin, &form).to_string()).unwrap();
        assert!(
            result["error"]
                .as_str()
                .is_some_and(|message| !message.is_empty())
        );
        assert!(
            result.get("fields").is_none(),
            "refusal must not manufacture a DNA form"
        );
        assert_eq!(state(&controller, original), before);
        controller
            .as_mut()
            .run_form(&plugin, &form, &QString::from("{}"));
        assert_eq!(controller.rust().message_kind.to_string(), "error");
        assert_eq!(
            controller.rust().message.to_string(),
            result["error"].as_str().unwrap()
        );
        controller.as_mut().select_all();
        assert!(!controller.as_mut().key(0x41, &QString::from("a"), CTRL));
        controller.as_mut().press(10.0, 10.0, 0);
        assert!(controller.as_mut().context_press(10.0, 10.0).is_empty());
        controller
            .as_mut()
            .find_query(&QString::from("replacement"), true);
        assert!(controller.as_mut().find_poll(1).is_empty());
        assert!(controller.as_mut().review_snapshot().is_empty());
        assert_eq!(state(&controller, original), before);
    }
    controller.as_mut().rust_mut().doc = Some(original);
    assert_eq!(controller.tool_form(&plugin, &form).to_string(), valid);
    assert_eq!(std::fs::read_to_string(path).unwrap(), content);
    assert_eq!(
        std::fs::read_to_string(sequence).unwrap(),
        ">sequence\nACGT\n"
    );
    println!("Alignment owned-controller wrong-kind/missing-ID refusal passed");
}
