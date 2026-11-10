//! Imported support values keep a verifiable, referenced (not copied) source.
use basecraft_ops::{Session, provenance, replay};
use basecraft_protocol::Op;
use std::path::Path;

fn paths(root: &Path) -> basecraft_platform::paths::AppPaths {
    basecraft_platform::paths::AppPaths::from_environment(|key| {
        (key == "BASECRAFT_PROFILE_DIR").then(|| root.join("profile").into_os_string())
    })
    .unwrap()
}

fn trees(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let target = project.join("target.nwk");
    std::fs::write(&target, "((A:1,B:1)90:1,(C:1,D:1)80:1);").unwrap();
    let source = project.join("support.nwk");
    std::fs::write(&source, "((A:1,B:1)75:1,(C:1,D:1)60:1);").unwrap();
    (target, source)
}

#[test]
fn imported_source_is_referenced_verified_and_survives_a_folder_move() {
    let dir = tempfile::tempdir().unwrap();
    let (target, source) = trees(dir.path());
    let paths = paths(dir.path());
    let mut session = Session::recoverable_with_paths(target.parent().unwrap(), &paths).unwrap();
    let doc = session.open(&target).unwrap();
    let imported = session.import_support(doc, &source, "Bootstrap").unwrap();
    assert_eq!(imported.added, vec!["Bootstrap".to_string()]);
    assert_eq!(imported.matched, 2);
    let set = session
        .tree(doc)
        .unwrap()
        .support("Bootstrap")
        .unwrap()
        .clone();
    assert_eq!(set.source, "support.nwk");
    // The activity wrote no file, so its history row names none.
    let report = session.document_record(doc).unwrap().report().unwrap();
    assert!(
        report
            .history
            .iter()
            .any(|row| row.summary == "Ran basecraft.tree import_support"),
        "{:?}",
        report.history
    );
    // Values and recorded activity form one undo step; redo restores them.
    session.undo(doc).unwrap();
    assert!(session.tree(doc).unwrap().support("Bootstrap").is_none());
    session.redo(doc).unwrap();
    assert_eq!(session.tree(doc).unwrap().support("Bootstrap"), Some(&set));
    session.save(doc, None).unwrap();
    drop(session);

    let publication = provenance::inspect(&target).unwrap();
    assert!(publication.events.iter().any(|event| matches!(&event.op,
        Op::ToolRun { plugin, operation, params, .. }
            if plugin == "basecraft.tree" && operation == "import_support"
                && params["input_run"]["id"].is_string() && params["sets"][0] == "Bootstrap")));
    assert_eq!(provenance::verify(&target).unwrap(), Vec::<String>::new());
    // Reference-first: the source bytes are not copied into any store.
    let source_hash = basecraft_io::hash_file(&source).unwrap();
    let copied = walk(&target.parent().unwrap().join(".basecraft"))
        .into_iter()
        .chain(walk(&provenance::companion(&target)))
        .any(|path| basecraft_io::hash_file(&path).ok().as_ref() == Some(&source_hash));
    assert!(!copied, "the support source was copied");

    // Moving the whole folder keeps the relative reference.
    let moved = dir.path().join("moved");
    std::fs::rename(target.parent().unwrap(), &moved).unwrap();
    let target = moved.join("target.nwk");
    assert_eq!(provenance::verify(&target).unwrap(), Vec::<String>::new());
    // Reopening keeps the imported values and their names.
    let reopened = basecraft_ops::files::load_tree(&target).unwrap();
    assert_eq!(
        reopened.doc.support("Bootstrap").map(|s| &s.values),
        Some(&set.values)
    );

    // A changed source is reported, never silently accepted.
    std::fs::write(moved.join("support.nwk"), "((A:1,B:1)10:1,(C:1,D:1)20:1);").unwrap();
    assert_ne!(provenance::verify(&target).unwrap(), Vec::<String>::new());
}

#[test]
fn a_source_without_support_or_a_clashing_name_changes_and_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (target, source) = trees(dir.path());
    let plain = target.parent().unwrap().join("plain.nwk");
    std::fs::write(&plain, "((A:1,B:1):1,(C:1,D:1):1);").unwrap();
    let paths = paths(dir.path());
    let mut session = Session::recoverable_with_paths(target.parent().unwrap(), &paths).unwrap();
    let doc = session.open(&target).unwrap();
    let before = session.tree(doc).unwrap().supports().to_vec();
    assert_eq!(
        session.import_support(doc, &plain, "").unwrap().added,
        Vec::<String>::new()
    );
    let existing = before[0].name.clone();
    assert!(
        session
            .import_support(doc, &source, &existing)
            .unwrap_err()
            .to_string()
            .contains("already a support set")
    );
    let other = target.parent().unwrap().join("other.nwk");
    std::fs::write(&other, "((A:1,E:1)75:1,(C:1,D:1)60:1);").unwrap();
    assert!(session.import_support(doc, &other, "x").is_err());
    assert_eq!(session.tree(doc).unwrap().supports(), before.as_slice());
    assert!(!session.can_undo(doc));
    assert!(!target.parent().unwrap().join(".basecraft").exists());
}

#[test]
fn imported_values_replay_from_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let (target, source) = trees(dir.path());
    let root = target.parent().unwrap();
    {
        let mut session = Session::new(root);
        let doc = session.open(&target).unwrap();
        session.import_support(doc, &source, "Imported").unwrap();
        session.save(doc, None).unwrap();
    }
    // Replay never rereads the live source: change it first.
    std::fs::write(&source, "((A:1,B:1)1:1,(C:1,D:1)2:1);").unwrap();
    let report = replay::replay(root).unwrap();
    assert!(report.ok(), "{:#?}", report.problems);
}

fn walk(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}
