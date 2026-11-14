//! Independent REV-0004 probes: failures describe the required persistence contract.
use basecraft_ops::records::AlignmentLinkExt;
use basecraft_ops::{Session, files, records, records::AlignmentLink};

fn write_alignment(path: &std::path::Path, bases: &str) {
    std::fs::write(path, format!(">a\n{bases}\n>b\n{bases}\n")).unwrap();
}

#[test]
fn save_as_replaces_destination_view_association() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    let input = root.join("input.nwk");
    let output = root.join("output.nwk");
    write_alignment(&source, "AA");
    write_alignment(&other, "CC");
    for tree in [&input, &output] {
        std::fs::write(tree, "(a,b);").unwrap();
        AlignmentLink::create(tree, &source, 3, true).unwrap();
    }
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    AlignmentLink::associate(&output, session.tree(doc).unwrap(), &other).unwrap();
    session.save(doc, Some(&output)).unwrap();
    let view = AlignmentLink::read_view(&output).unwrap();
    assert!(
        view.checked_alignment(&output, session.tree(doc).unwrap())
            .is_ok()
    );
    assert_eq!(
        view.source, source,
        "successful Save As must not retain the overwritten tree's unrelated view link"
    );
}

#[test]
fn failed_save_as_does_not_expose_origin_as_explicit_view() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    let input = root.join("input.nwk");
    let output = root.join("output.nwk");
    write_alignment(&source, "AA");
    write_alignment(&other, "CC");
    std::fs::write(&input, "(a,b);").unwrap();
    AlignmentLink::create(&input, &source, 3, true).unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    AlignmentLink::associate(&input, session.tree(doc).unwrap(), &other).unwrap();
    // The output's companion cannot be written, so Save As stops part way.
    std::fs::create_dir_all(files::companion_path(None, &output)).unwrap();
    let error = session.save(doc, Some(&output)).unwrap_err();
    eprintln!("save error: {error}");
    assert!(
        AlignmentLink::read_view(&input)
            .unwrap()
            .checked_alignment(&input, session.tree(doc).unwrap())
            .is_ok()
    );
    if output.exists() {
        let reopened = files::load_tree(&output).unwrap().doc;
        assert!(
            AlignmentLink::read_view(&output)
                .and_then(|link| link.checked_alignment(&output, &reopened))
                .is_err(),
            "partial Save As must not silently substitute scientific origin for the damaged explicit view"
        );
    }
}

#[cfg(unix)]
#[test]
fn dangling_explicit_view_link_refuses_fallback() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let input = root.join("input.nwk");
    write_alignment(&source, "AA");
    std::fs::write(&input, "(a,b);").unwrap();
    AlignmentLink::create(&input, &source, 3, true).unwrap();
    std::os::unix::fs::symlink("missing-view.json", records::view_sidecar(&input)).unwrap();
    assert!(
        AlignmentLink::read_view(&input).is_err(),
        "an unreadable explicit association must not fall back to scientific origin"
    );
}

#[test]
fn failed_view_replacement_preserves_retained_snapshot() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    let replacement = root.join("replacement.afa");
    let input = root.join("input.nwk");
    write_alignment(&source, "AA");
    write_alignment(&other, "CC");
    write_alignment(&replacement, "GG");
    std::fs::write(&input, "(a,b);").unwrap();
    AlignmentLink::create(&input, &source, 3, true).unwrap();
    let tree = files::load_tree(&input).unwrap().doc;
    AlignmentLink::associate(&input, &tree, &other).unwrap();
    let old = AlignmentLink::read_view(&input).unwrap();
    let snapshot = old.checked_alignment(&input, &tree).unwrap();
    let bytes = std::fs::read(&snapshot).unwrap();
    // Force metadata replacement to fail after the snapshot write.
    let metadata = records::view_sidecar(&input);
    let backup = root.join("view-metadata-backup.json");
    std::fs::rename(&metadata, &backup).unwrap();
    std::fs::create_dir(&metadata).unwrap();
    assert!(AlignmentLink::associate(&input, &tree, &replacement).is_err());
    std::fs::remove_dir(&metadata).unwrap();
    std::fs::rename(&backup, &metadata).unwrap();
    assert_eq!(std::fs::read(snapshot).unwrap(), bytes);
    assert!(old.checked_alignment(&input, &tree).is_ok());
}
