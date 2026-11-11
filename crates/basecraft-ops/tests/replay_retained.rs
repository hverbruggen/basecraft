//! Repeated baselines must never mutate retained inputs or original sources.
use std::{collections::BTreeMap, path::Path};

use basecraft_ops::{Session, delimitation, replay};
use basecraft_protocol::{Op, delimitation::DelimitationEdit};

const SPART: &str = include_str!("../../../testdata/fixtures/delimitation/manual.spart");

fn fingerprint(path: &Path) -> (u64, String) {
    (
        path.metadata().unwrap().len(),
        basecraft_io::hash_file(path).unwrap(),
    )
}

/// Every retained file: checkpoints in objects/, recovery links in originals/.
fn objects(root: &Path) -> BTreeMap<String, (u64, String)> {
    let mut found = BTreeMap::new();
    let mut pending = vec![
        root.join(".basecraft/objects"),
        root.join(".basecraft/originals"),
    ];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let key = path.strip_prefix(root).unwrap().display().to_string();
                found.insert(key, fingerprint(&path));
            }
        }
    }
    found
}

fn qualify(root: &Path, recovered: &Path) {
    let before = objects(root);
    assert!(!before.is_empty());
    println!("retained objects before replay/recovery: {before:?}");
    for _ in 0..2 {
        let report = replay::replay(root).unwrap();
        assert!(report.ok(), "{report:#?}");
        assert_eq!(objects(root), before);
    }
    let outputs = replay::recover(root, recovered).unwrap();
    assert_eq!(outputs, vec![recovered.join("a.spart")]);
    assert_eq!(objects(root), before);
    println!(
        "retained objects after replay/recovery: {:?}",
        objects(root)
    );
}

#[test]
fn repeated_open_with_edits_preserves_inputs_in_replay_and_recovery() {
    // Successful recovery registers its final store. Keep each fixture alive
    // until the other cases finish inspecting that shared test profile.
    let mut retained = Vec::new();
    for source_state in ["unchanged", "changed", "missing"] {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let path = root.join("a.spart");
        std::fs::write(&path, SPART).unwrap();
        let original = fingerprint(&path);
        let mut session = Session::new(&root);
        for _ in 0..2 {
            let doc = session.open(&path).unwrap();
            session
                .apply(
                    doc,
                    Op::DelimitationEdit {
                        edit: Box::new(DelimitationEdit::Remove {
                            hypotheses: ["partition-3".into()].into(),
                        }),
                    },
                )
                .unwrap();
        }
        session.flush().unwrap();
        drop(session);
        assert_eq!(fingerprint(&path), original);
        match source_state {
            // Editors replace by rename; a hard link keeps the old inode. An
            // in-place rewrite after the session ends cannot be recovered
            // without a private copy.
            "changed" => {
                let replacement = root.join("replacement.tmp");
                std::fs::write(&replacement, "changed source").unwrap();
                std::fs::rename(&replacement, &path).unwrap();
            }
            "missing" => std::fs::remove_file(&path).unwrap(),
            _ => (),
        }
        let source = path.exists().then(|| fingerprint(&path));
        let recovered = parent.path().join("recovered");
        qualify(&root, &recovered);
        assert_eq!(path.exists().then(|| fingerprint(&path)), source);
        let loaded = basecraft_ops::files::load_delimitation(&recovered.join("a.spart")).unwrap();
        assert_eq!(loaded.hypotheses.len(), 2);
        retained.push(parent);
    }
}

#[test]
fn repeated_combine_save_and_refused_save_share_one_session() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let a = root.join("a.spart");
    let b = root.join("b.spart");
    std::fs::write(&a, SPART).unwrap();
    std::fs::write(&b, SPART).unwrap();
    let original = fingerprint(&a);
    let mut session = Session::new(&root);
    for name in ["one.spart", "two.spart", "refused.spart"] {
        let doc = session.open(&a).unwrap();
        delimitation::combine(&mut session, doc, std::slice::from_ref(&b)).unwrap();
        assert_eq!(session.delimitation(doc).unwrap().hypotheses.len(), 6);
        let output = root.join(name);
        if name == "refused.spart" {
            // A directory is a deterministic disk-level save refusal.
            std::fs::create_dir(&output).unwrap();
            assert!(session.save(doc, Some(&output)).is_err());
        } else {
            session.save(doc, Some(&output)).unwrap();
        }
    }
    session.flush().unwrap();
    drop(session);
    let recovered = parent.path().join("recovered");
    qualify(&root, &recovered);
    assert_eq!(fingerprint(&a), original);
    assert_eq!(fingerprint(&b), original);
    let loaded = basecraft_ops::files::load_delimitation(&recovered.join("a.spart")).unwrap();
    assert_eq!(loaded.hypotheses.len(), 6);
}
