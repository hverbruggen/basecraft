//! Failure and overwrite coverage supplementing the unchanged REV-0004 probes.
use basecraft_ops::records::AlignmentLinkExt;
use basecraft_ops::{Session, files, records, records::AlignmentLink};
use std::path::{Path, PathBuf};

fn alignment(path: &Path, bases: &str) {
    std::fs::write(path, format!(">a\n{bases}\n>b\n{bases}\n")).unwrap();
}

fn inferred(root: &Path, name: &str, source: &Path) -> PathBuf {
    let path = root.join(format!("{name}.nwk"));
    std::fs::write(&path, "(a,b);").unwrap();
    AlignmentLink::create(&path, source, 3, true).unwrap();
    path
}

#[test]
fn a_failed_relink_keeps_the_earlier_link() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    let replacement = root.join("replacement.afa");
    alignment(&source, "AA");
    alignment(&other, "CC");
    alignment(&replacement, "GG");
    let path = inferred(root, "tree", &source);
    let tree = files::load_tree(&path).unwrap().doc;
    AlignmentLink::associate(&path, &tree, &other).unwrap();
    let metadata = records::view_sidecar(&path);
    let original_metadata = std::fs::read(&metadata).unwrap();
    let backup = root.join("old-view.json");
    std::fs::rename(&metadata, &backup).unwrap();
    std::fs::create_dir(&metadata).unwrap();
    assert!(AlignmentLink::associate(&path, &tree, &replacement).is_err());
    std::fs::remove_dir(&metadata).unwrap();
    std::fs::rename(backup, &metadata).unwrap();
    let restored = AlignmentLink::read_view(&path).unwrap();
    assert_eq!(
        restored.checked_alignment(None, &path, &tree).unwrap(),
        other.canonicalize().unwrap()
    );
    assert_eq!(std::fs::read(&metadata).unwrap(), original_metadata);
    AlignmentLink::associate(&path, &tree, &replacement).unwrap();
    assert_eq!(AlignmentLink::read_view(&path).unwrap().source, replacement);
}

#[test]
fn save_as_unlinked_document_clears_both_destination_associations_and_keeps_evidence() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    alignment(&source, "AA");
    alignment(&other, "CC");
    let input = root.join("input.nwk");
    std::fs::write(&input, "(a,b);").unwrap();
    let output = inferred(root, "output", &source);
    let tree = files::load_tree(&input).unwrap().doc;
    AlignmentLink::associate(&output, &tree, &other).unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    session.save(doc, Some(&output)).unwrap();
    assert!(!records::metadata_present(&records::sidecar(&output)).unwrap());
    assert!(!records::metadata_present(&records::view_sidecar(&output)).unwrap());
    assert!(AlignmentLink::read_view(&output).is_err());
    assert_eq!(std::fs::read_to_string(&other).unwrap(), ">a\nCC\n>b\nCC\n");
    assert!(basecraft_ops::records::replay(root).unwrap().ok());
}

#[test]
fn publication_error_after_main_write_disables_extraction_and_keeps_previous_association_record() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    alignment(&source, "AA");
    alignment(&other, "CC");
    let input = inferred(root, "input", &source);
    let output = inferred(root, "output", &source);
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    AlignmentLink::associate(&output, session.tree(doc).unwrap(), &other).unwrap();
    let old = AlignmentLink::read_view(&output).unwrap();
    let old_snapshot = old
        .checked_alignment(None, &output, session.tree(doc).unwrap())
        .unwrap();
    let old_bytes = std::fs::read(&old_snapshot).unwrap();
    let old_metadata = std::fs::read(records::view_sidecar(&output)).unwrap();
    // Tree replacement succeeds, then presentation-sidecar replacement fails.
    std::fs::create_dir_all(files::companion_path(None, &output)).unwrap();
    let error = session.save(doc, Some(&output)).unwrap_err().to_string();
    assert!(error.contains("Save As incomplete"), "{error}");
    assert!(error.contains("recovery metadata at"), "{error}");
    let link_error = AlignmentLink::read_view(&output).unwrap_err().to_string();
    assert!(
        link_error.contains("publication incomplete"),
        "{link_error}"
    );
    assert_eq!(std::fs::read(&old_snapshot).unwrap(), old_bytes);
    let record = std::fs::read_dir(records::sidecar(&output).parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".alignment-recovery.json"))
        .unwrap();
    let recovery: serde_json::Value =
        serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
    assert_eq!(
        recovery["previous_view"]["value"],
        serde_json::to_value(old_metadata).unwrap()
    );
    assert_eq!(AlignmentLink::read(&input).unwrap().source, source);
}

/// D25: a linked alignment is checked when it is used, not when the tree is
/// saved; Save As carries the link and the change shows on use.
#[test]
fn a_changed_linked_alignment_shows_on_use_after_save_as() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    alignment(&source, "AA");
    alignment(&other, "CC");
    let input = inferred(root, "input", &source);
    let output = root.join("output.nwk");
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    AlignmentLink::associate(&input, session.tree(doc).unwrap(), &other).unwrap();
    alignment(&other, "GG");
    session.save(doc, Some(&output)).unwrap();
    let view = AlignmentLink::read_view(&output).unwrap();
    assert_eq!(view.source, other);
    assert!(matches!(
        AlignmentLink::checked_view_in(None, &output, session.tree(doc).unwrap()),
        Err(basecraft_ops::Error::AlignmentChanged { .. })
    ));
}

#[cfg(unix)]
#[test]
fn dangling_explicit_metadata_blocks_save_as_and_is_reported_without_changes() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    alignment(&source, "AA");
    let input = inferred(root, "input", &source);
    let output = root.join("output.nwk");
    let metadata = records::view_sidecar(&input);
    std::os::unix::fs::symlink("missing.json", &metadata).unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    assert!(records::metadata_present(&metadata).unwrap());
    assert!(session.save(doc, Some(&output)).is_err());
    assert!(!output.exists());
    assert_eq!(
        std::fs::read_link(&metadata).unwrap(),
        Path::new("missing.json")
    );
    assert!(AlignmentLink::read_view(&input).is_err());
}

#[test]
fn save_as_with_explicit_view_remains_portable_and_preserves_source_hashes() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let folder = root.join("project");
    std::fs::create_dir(&folder).unwrap();
    let source = folder.join("source.afa");
    let other = folder.join("other.afa");
    alignment(&source, "AA");
    alignment(&other, "CC");
    let input = inferred(&folder, "input", &source);
    let mut session = Session::new(&folder);
    let doc = session.open(&input).unwrap();
    AlignmentLink::associate(&input, session.tree(doc).unwrap(), &other).unwrap();
    let original = AlignmentLink::read(&input).unwrap();
    let original_metadata = std::fs::read(records::sidecar(&input)).unwrap();
    let output = folder.join("copy.nwk");
    session.save(doc, Some(&output)).unwrap();
    assert_eq!(
        std::fs::read(records::sidecar(&input)).unwrap(),
        original_metadata
    );
    assert!(basecraft_ops::records::replay(&folder).unwrap().ok());
    drop(session);
    let moved = root.join("moved");
    std::fs::rename(folder, &moved).unwrap();
    let path = moved.join("copy.nwk");
    let tree = files::load_tree(&path).unwrap().doc;
    let origin = AlignmentLink::read(&path).unwrap();
    let view = AlignmentLink::read_view(&path).unwrap();
    assert_eq!(origin.source, moved.join("source.afa"));
    assert_eq!(view.source, moved.join("other.afa"));
    assert_eq!(origin.source_hash, original.source_hash);
    assert_eq!(origin.alignment_version, original.alignment_version);
    assert!(origin.checked_alignment(None, &path, &tree).is_ok());
    assert!(view.checked_alignment(None, &path, &tree).is_ok());
}

#[cfg(unix)]
#[test]
fn save_as_over_symlink_to_original_replaces_alias_associations() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("source.afa");
    let other = root.join("other.afa");
    alignment(&source, "AA");
    alignment(&other, "CC");
    let input = inferred(root, "input", &source);
    let output = root.join("alias.nwk");
    std::os::unix::fs::symlink(&input, &output).unwrap();
    AlignmentLink::create(&output, &other, 3, true).unwrap();
    let mut session = Session::new(root);
    let doc = session.open(&input).unwrap();
    AlignmentLink::associate(&output, session.tree(doc).unwrap(), &other).unwrap();
    let previous_origin = std::fs::read(records::sidecar(&input)).unwrap();
    session.save(doc, Some(&output)).unwrap();
    assert!(
        !std::fs::symlink_metadata(&output)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let link = AlignmentLink::read_view(&output).unwrap();
    assert_eq!(link.source, source);
    assert!(
        link.checked_alignment(None, &output, session.tree(doc).unwrap())
            .is_ok()
    );
    assert_eq!(
        std::fs::read(records::sidecar(&input)).unwrap(),
        previous_origin
    );
    assert!(basecraft_ops::records::replay(root).unwrap().ok());
}
