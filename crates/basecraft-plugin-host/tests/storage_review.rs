//! Independent combined-storage lifecycle regression probes, on registered runs.
// Preserve the independently reviewed assertions verbatim.
#![allow(clippy::assert_is_empty)]
mod common;

use basecraft_ops::{Session, analysis_store::ArtifactRun, input_evidence, records};
use basecraft_plugin_host::registry::Registry;
use basecraft_protocol::Op;
use std::path::{Path, PathBuf};

fn analyse(session: &mut Session, registry: &Registry, source: &Path, output: &Path) {
    common::align_copy(registry, session, source, output);
}

fn setup() -> (tempfile::TempDir, Session, Registry, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.afa");
    std::fs::write(&source, ">a\nAAAA\n>b\nAAAT\n").unwrap();
    let first = dir.path().join("first.afa");
    let registry = common::copy_aligner(dir.path());
    let mut session = Session::recoverable(dir.path()).unwrap();
    analyse(&mut session, &registry, &source, &first);
    (dir, session, registry, source, first)
}

fn save_as(session: &mut Session, first: &Path, copied: &Path) {
    let doc = session.open(first).unwrap();
    session.save(doc, Some(copied)).unwrap();
    assert!(records::verify(copied).unwrap().is_empty());
}

#[test]
fn dependencies_follow_save_as_delegation() {
    let (dir, mut session, registry, source, first) = setup();
    let copied = dir.path().join("copy.afa");
    save_as(&mut session, &first, &copied);
    let downstream = dir.path().join("downstream.afa");
    analyse(&mut session, &registry, &copied, &downstream);
    let downstream_run = ArtifactRun::for_result(&downstream).unwrap().unwrap();
    let conflict = input_evidence::conflicts(&source).unwrap();
    assert!(
        conflict.analyses.iter().any(|a| a.run == downstream_run),
        "saving the original must list downstream evidence through Save As: {conflict:?}"
    );
}

#[test]
fn relinking_saved_copy_repairs_the_effective_reference() {
    let (dir, mut session, _, source, first) = setup();
    let copied = dir.path().join("copy.afa");
    save_as(&mut session, &first, &copied);
    let recovered = dir.path().join("recovered.afa");
    std::fs::rename(&source, &recovered).unwrap();
    let bundle = records::bundle_for_result(&copied);
    input_evidence::relink(&bundle, &recovered).unwrap();
    let issues = records::verify(&copied).unwrap();
    assert!(
        issues.is_empty(),
        "successful relink must repair the effective evidence: {issues:?}"
    );
}

#[test]
fn complete_export_of_saved_copy_is_independent() {
    let (dir, mut session, _, source, first) = setup();
    let copied = dir.path().join("copy.afa");
    save_as(&mut session, &first, &copied);
    let package = dir.path().join("portable");
    records::package(&copied, &package).expect("Save As result must support complete export");
    std::fs::remove_file(&source).unwrap();
    let issues = records::verify(&package.join("copy.afa")).unwrap();
    assert!(
        issues.is_empty(),
        "complete export still depends on original files: {issues:?}"
    );
}

#[test]
fn complete_export_after_in_place_edit_keeps_history() {
    let (dir, mut session, _, source, first) = setup();
    let doc = session.open(&first).unwrap();
    session
        .apply(
            doc,
            Op::AlnReplace {
                row: 0,
                start: 0,
                end: 1,
                residues: "T".into(),
            },
        )
        .unwrap();
    session.save(doc, None).unwrap();
    assert!(records::verify(&first).unwrap().is_empty());
    let package = dir.path().join("portable");
    records::package(&first, &package)
        .expect("edited analysis result must support complete export");
    std::fs::remove_file(&source).unwrap();
    let issues = records::verify(&package.join("first.afa")).unwrap();
    assert!(
        issues.is_empty(),
        "complete export lost evidence: {issues:?}"
    );
}

#[test]
fn repeated_preserve_decision_never_replaces_verified_old_bytes() {
    let (_dir, _session, _, source, first) = setup();
    let old = std::fs::read(&source).unwrap();
    let conflict = input_evidence::conflicts(&source).unwrap();
    input_evidence::resolve(&conflict, input_evidence::SavePolicy::PreserveCopy).unwrap();
    let run = ArtifactRun::for_result(&first).unwrap().unwrap();
    assert_eq!(std::fs::read(common::verified_input(&run)).unwrap(), old);
    std::fs::write(&source, ">a\nTTTT\n>b\nTTTA\n").unwrap();
    // A retry of the same public decision must not overwrite the protected copy,
    // even if it rejects the now-stale source afterwards.
    let _ = input_evidence::resolve(&conflict, input_evidence::SavePolicy::PreserveCopy);
    assert_eq!(std::fs::read(common::verified_input(&run)).unwrap(), old);
}

#[test]
fn overwrite_conflict_identifies_the_completed_result() {
    let (_dir, _session, _, source, _first) = setup();
    let conflict = input_evidence::conflicts(&source).unwrap();
    let analyses = common::analyses(&conflict);
    assert_eq!(analyses.len(), 1);
    assert_eq!(analyses[0].result, "first.afa");
}
