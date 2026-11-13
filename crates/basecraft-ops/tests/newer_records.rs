//! Records carry the basecraft version that wrote them, and a save that
//! would drop fields a later basecraft wrote asks first (D14).
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

use basecraft_ops::{DocId, Error, Session};
use basecraft_protocol::Op;
use serde_json::{Value, json};
use std::path::Path;

fn reroot(session: &mut Session, tree: DocId, tip: &str) {
    let op = Op::TreeReroot {
        clade: vec![tip.into()],
    };
    session.apply(tree, op).unwrap();
}

/// A saved tree whose side file is then rewritten as `writer` would write it,
/// with `extra` added; reopened and edited, ready to save.
fn edited_tree(root: &Path, writer: &str, extra: &Value) -> (Session, DocId) {
    let path = root.join("t.nwk");
    std::fs::write(&path, "[&R] ((a,b),(c,d));\n").unwrap();
    let mut session = Session::new(root);
    let tree = session.open(&path).unwrap();
    reroot(&mut session, tree, "b");
    session.save(tree, None).unwrap();
    drop(session);
    let side = root.join("t.nwk.basecraft.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&side).unwrap()).unwrap();
    assert_eq!(record["basecraft"], basecraft_protocol::VERSION);
    record["basecraft"] = writer.into();
    for (key, value) in extra.as_object().unwrap() {
        record[key] = value.clone();
    }
    std::fs::write(&side, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let mut session = Session::new(root);
    let tree = session.open(&path).unwrap();
    reroot(&mut session, tree, "c");
    (session, tree)
}

fn side_file(root: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(root.join("t.nwk.basecraft.json")).unwrap()).unwrap()
}

#[test]
fn a_save_that_would_drop_newer_fields_is_refused_until_allowed() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, tree) = edited_tree(dir.path(), "99.0.0", &json!({"future": {"kept": 1}}));
    let error = session.save(tree, None).unwrap_err();
    assert_eq!(error.code(), "newer_fields_dropped");
    let message = error.to_string();
    for part in ["99.0.0", basecraft_protocol::VERSION, "future", "Save as"] {
        assert!(message.contains(part), "{message}");
    }
    assert!(matches!(&error, Error::NewerFieldsDropped { fields, .. } if fields == &["future"]));
    // Nothing was replaced.
    assert_eq!(side_file(dir.path())["future"], json!({"kept": 1}));

    // Save As to another file drops nothing there.
    session
        .save_new(tree, Some(&dir.path().join("copy.nwk")))
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let (mut session, tree) = edited_tree(dir.path(), "99.0.0", &json!({"future": {"kept": 1}}));
    session.drop_newer_fields(tree).unwrap();
    session.save(tree, None).unwrap();
    let saved = side_file(dir.path());
    assert_eq!(saved["basecraft"], basecraft_protocol::VERSION);
    assert!(saved.get("future").is_none());
}

#[test]
fn a_newer_record_with_nothing_unknown_saves_silently() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, tree) = edited_tree(dir.path(), "1.0.0-alpha.1", &json!({}));
    session.save(tree, None).unwrap();
    assert_eq!(
        side_file(dir.path())["basecraft"],
        basecraft_protocol::VERSION
    );
}

#[test]
fn unknown_fields_from_the_same_or_an_older_version_save_silently() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, tree) = edited_tree(dir.path(), "0.1.0", &json!({"future": 1}));
    session.save(tree, None).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, tree) = edited_tree(
        dir.path(),
        basecraft_protocol::VERSION,
        &json!({"future": 1}),
    );
    session.save(tree, None).unwrap();
}

#[test]
fn journal_lines_name_the_version_that_wrote_them() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let (mut session, tree) = edited_tree(dir.path(), basecraft_protocol::VERSION, &json!({}));
    session.save(tree, None).unwrap();
    let mut lines = 0;
    for entry in walk(&dir.path().join(".basecraft")) {
        if entry.extension().is_some_and(|e| e == "jsonl") {
            for line in std::fs::read_to_string(&entry).unwrap().lines() {
                let value: Value = serde_json::from_str(line).unwrap();
                assert_eq!(value["basecraft"], basecraft_protocol::VERSION);
                lines += 1;
            }
        }
    }
    assert!(lines > 0);
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}
