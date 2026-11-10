//! M3 step 1: Session-level guards across store locations and file identities.
//! These pin existing protections, not the future registry/locking policy.
use std::{path::Path, process::Command};

use basecraft_ops::{
    DocId, Error, Session,
    analysis_store::ArtifactRun,
    input_evidence::{self, EvidenceConflict, SavePolicy},
    replay,
};
use basecraft_platform::paths::AppPaths;
use basecraft_protocol::Op;

const ORIGINAL: &[u8] = b">gene\nACGT\n";

// Reuse the budget/kind-conformance child-process pattern. Command::env selects
// the profile before any AppPaths/Session initialization; no process-global env
// mutation or serial lock is needed in the parallel parent harness.
fn isolated(name: &str, test: impl FnOnce(&Path)) {
    if std::env::var("BASECRAFT_REFERENCE_CHILD").as_deref() != Ok(name) {
        let root = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("BASECRAFT_REFERENCE_CHILD", name)
            .env("BASECRAFT_REFERENCE_ROOT", root.path())
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .env("BASECRAFT_STATE_DIR", root.path().join("profile/state"))
            .env("BASECRAFT_CACHE_DIR", root.path().join("profile/cache"))
            .env("BASECRAFT_SCRATCH_DIR", root.path().join("scratch"))
            .env("TMPDIR", root.path())
            .env("TMP", root.path())
            .env("TEMP", root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("BASECRAFT_REFERENCE_ROOT").unwrap());
    assert_eq!(
        AppPaths::discover().unwrap().state,
        root.join("profile/state")
    );
    test(&root);
}

fn folder(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn reference(root: &Path, source: &Path) -> ArtifactRun {
    input_evidence::reference_input_with_dependencies(
        &root.join("result.txt"),
        source,
        &basecraft_io::hash_file(source).unwrap(),
        &[],
        "save protection regression",
    )
    .unwrap()
}

fn edited(root: &Path, file: &Path) -> (Session, DocId) {
    let mut session = Session::recoverable(root).unwrap();
    let doc = session.open(file).unwrap();
    session
        .apply(
            doc,
            Op::SeqReplace {
                start: 1,
                end: 2,
                residues: "T".into(),
            },
        )
        .unwrap();
    (session, doc)
}

fn conflict(error: Error) -> EvidenceConflict {
    match error {
        Error::EvidenceConflict(conflict) => *conflict,
        other => panic!("expected structured evidence conflict, got {other:?}"),
    }
}

fn recoverable(session: &mut Session, doc: DocId, destination: &Path) {
    assert!(session.document_record(doc).unwrap().unsaved);
    assert!(session.can_undo(doc));
    session.flush().unwrap();
    let outputs = replay::recover(session.recovery_root(), destination).unwrap();
    assert_eq!(outputs.len(), 1);
    assert!(
        std::fs::read_to_string(&outputs[0])
            .unwrap()
            .contains("ATGT")
    );
}

#[test]
fn registered_other_folder_policies_preserve_original_and_pending_edits() {
    isolated(
        "registered_other_folder_policies_preserve_original_and_pending_edits",
        |root| {
            for (index, policy) in [
                SavePolicy::Cancel,
                SavePolicy::PreserveCopy,
                SavePolicy::BreakEvidence,
            ]
            .into_iter()
            .enumerate()
            {
                let data = folder(root, &format!("data-{index}"));
                let elsewhere = folder(root, &format!("other-project-{index}"));
                let source = data.join("gene.fasta");
                std::fs::write(&source, ORIGINAL).unwrap();
                let run = reference(&elsewhere, &source);
                let (mut session, doc) = edited(&data, &source);
                let generation = session.generation(doc);
                // The store is not an ancestor of the target: only registration
                // makes this conflict visible to Session.
                let found = conflict(session.save(doc, None).unwrap_err());
                assert_eq!(found.analyses.len(), 1);
                assert_eq!(found.analyses[0].run, run.directory());
                assert_eq!(session.generation(doc), generation);
                assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
                recoverable(&mut session, doc, &root.join(format!("recovered-{index}")));
                if policy == SavePolicy::Cancel {
                    conflict(session.save_with_evidence(doc, None, policy).unwrap_err());
                    assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
                    assert_eq!(
                        input_evidence::read_input(run.directory()).unwrap().state,
                        "referenced"
                    );
                    continue;
                }
                session.save_with_evidence(doc, None, policy).unwrap();
                assert!(std::fs::read_to_string(&source).unwrap().contains("ATGT"));
                let input = input_evidence::read_input(run.directory()).unwrap();
                if policy == SavePolicy::PreserveCopy {
                    let retained = input_evidence::verified_source(
                        run.directory(),
                        input.source.as_ref().unwrap(),
                    )
                    .unwrap();
                    assert_eq!(std::fs::read(retained).unwrap(), ORIGINAL);
                    assert_eq!(input.state, "preserved");
                } else {
                    assert_eq!(input.state, "broken");
                    assert!(
                        input_evidence::verified_source(
                            run.directory(),
                            input.source.as_ref().unwrap()
                        )
                        .is_err()
                    );
                }
            }
        },
    );
}

#[cfg(unix)]
#[test]
fn session_save_as_aliases_cannot_bypass_registered_input_guard() {
    isolated(
        "session_save_as_aliases_cannot_bypass_registered_input_guard",
        |root| {
            let data = folder(root, "data");
            let elsewhere = folder(root, "other-project");
            let source = data.join("source.fasta");
            let editing = data.join("editing.fasta");
            std::fs::write(&source, ORIGINAL).unwrap();
            std::fs::write(&editing, ORIGINAL).unwrap();
            let run = reference(&elsewhere, &source);
            let hard = data.join("hard.fasta");
            let soft = data.join("soft.fasta");
            std::fs::hard_link(&source, &hard).unwrap();
            std::os::unix::fs::symlink(&source, &soft).unwrap();
            let (mut session, doc) = edited(&data, &editing);
            for alias in [&hard, &soft] {
                let found = conflict(session.save(doc, Some(alias)).unwrap_err());
                assert_eq!(found.analyses.len(), 1);
                assert_eq!(found.analyses[0].run, run.directory());
                assert_eq!(std::fs::read(alias).unwrap(), ORIGINAL);
                assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
                assert!(session.document_record(doc).unwrap().unsaved);
            }
            recoverable(&mut session, doc, &root.join("recovered"));
            session
                .save_with_evidence(doc, Some(&hard), SavePolicy::PreserveCopy)
                .unwrap();
            assert_eq!(
                std::fs::read(run.directory().join("preserved-input")).unwrap(),
                ORIGINAL
            );
            assert!(std::fs::read_to_string(&hard).unwrap().contains("ATGT"));
            // Atomic replacement unlinks the hard-link destination; the original
            // inode and the symlink to it retain their bytes.
            assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
            assert_eq!(std::fs::read(&soft).unwrap(), ORIGINAL);
        },
    );
}

#[test]
fn moved_project_store_is_discovered_with_stale_registry_location() {
    isolated(
        "moved_project_store_is_discovered_with_stale_registry_location",
        |root| {
            let old = folder(root, "before-move");
            let source = old.join("gene.fasta");
            std::fs::write(&source, ORIGINAL).unwrap();
            let run = reference(&old, &source);
            let relative_run = run.directory().strip_prefix(&old).unwrap().to_path_buf();
            let moved = root.join("after-move");
            std::fs::rename(&old, &moved).unwrap();
            let source = moved.join("gene.fasta");
            let run = moved.join(relative_run);
            let (mut session, doc) = edited(&moved, &source);
            // The v1 registry cannot prove the missing old path is the same
            // store as the newly discovered nearby one. Preserve known guards
            // while explicitly acknowledging that remaining uncertainty.
            let check = match session.save(doc, None).unwrap_err() {
                Error::SaveCheckRequired(check) => check,
                other => panic!("expected incomplete moved-store check, got {other:?}"),
            };
            assert!(!check.evidence().is_complete());
            let found = &check.evidence().conflict;
            assert_eq!(found.analyses.len(), 1);
            assert_eq!(found.analyses[0].run, run);
            assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
            recoverable(&mut session, doc, &root.join("recovered"));
            session
                .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, true))
                .unwrap();
            assert_eq!(
                std::fs::read(run.join("preserved-input")).unwrap(),
                ORIGINAL
            );
            assert!(std::fs::read_to_string(source).unwrap().contains("ATGT"));
        },
    );
}

#[test]
fn lexical_old_path_still_guards_missing_or_replaced_source() {
    isolated(
        "lexical_old_path_still_guards_missing_or_replaced_source",
        |root| {
            for replaced in [false, true] {
                let data = folder(root, if replaced { "replaced" } else { "missing" });
                let elsewhere = folder(&data, "external-store");
                let source = data.join("source.fasta");
                let editing = data.join("editing.fasta");
                std::fs::write(&source, ORIGINAL).unwrap();
                std::fs::write(&editing, ORIGINAL).unwrap();
                let run = reference(&elsewhere, &source);
                let moved = data.join("moved.fasta");
                std::fs::rename(&source, &moved).unwrap();
                if replaced {
                    std::fs::write(&source, b">external\nTTTT\n").unwrap();
                }
                let before = std::fs::read(&source).ok();
                let (mut session, doc) = edited(&data, &editing);
                let found = conflict(session.save(doc, Some(&source)).unwrap_err());
                assert_eq!(found.analyses.len(), 1);
                assert_eq!(found.analyses[0].run, run.directory());
                assert!(!found.options.contains(&SavePolicy::PreserveCopy));
                assert!(
                    session
                        .save_with_evidence(doc, Some(&source), SavePolicy::PreserveCopy)
                        .is_err()
                );
                assert_eq!(std::fs::read(&source).ok(), before);
                assert_eq!(std::fs::read(&moved).unwrap(), ORIGINAL);
                recoverable(&mut session, doc, &data.join("recovered"));
                session
                    .save_with_evidence(doc, Some(&source), SavePolicy::BreakEvidence)
                    .unwrap();
                assert_eq!(
                    input_evidence::read_input(run.directory()).unwrap().state,
                    "broken"
                );
                assert!(std::fs::read_to_string(source).unwrap().contains("ATGT"));
                assert_eq!(std::fs::read(&moved).unwrap(), ORIGINAL);
            }
        },
    );
}

#[test]
fn registered_transitive_dependencies_are_included_in_session_conflict() {
    isolated(
        "registered_transitive_dependencies_are_included_in_session_conflict",
        |root| {
            let data = folder(root, "data");
            let elsewhere = folder(root, "other-project");
            let source = data.join("gene.fasta");
            std::fs::write(&source, ORIGINAL).unwrap();
            let parent = reference(&elsewhere, &source);
            let other = data.join("independent.fasta");
            std::fs::write(&other, b">other\nTTTT\n").unwrap();
            let dependent = |ancestor: &ArtifactRun| {
                input_evidence::reference_input_with_dependencies(
                    &elsewhere.join("dependent.txt"),
                    &other,
                    &basecraft_io::hash_file(&other).unwrap(),
                    &[ancestor.directory().to_owned()],
                    "dependent regression",
                )
                .unwrap()
            };
            let child = dependent(&parent);
            let grandchild = dependent(&child);
            let (mut session, doc) = edited(&data, &source);
            let found = conflict(session.save(doc, None).unwrap_err());
            assert_eq!(found.analyses.len(), 3);
            for (run, direct) in [(&parent, true), (&child, false), (&grandchild, false)] {
                assert!(
                    found
                        .analyses
                        .iter()
                        .any(|a| a.run == run.directory() && a.direct == direct)
                );
            }
            assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
            recoverable(&mut session, doc, &root.join("recovered"));
            session
                .save_with_evidence(doc, None, SavePolicy::PreserveCopy)
                .unwrap();
            assert_eq!(
                std::fs::read(parent.directory().join("preserved-input")).unwrap(),
                ORIGINAL
            );
            for run in [&child, &grandchild] {
                assert_eq!(
                    input_evidence::read_input(run.directory()).unwrap().state,
                    "referenced"
                );
                let reference = input_evidence::read_input(parent.directory())
                    .unwrap()
                    .source
                    .unwrap();
                let retained =
                    input_evidence::verified_source(run.directory(), &reference).unwrap();
                assert_eq!(std::fs::read(retained).unwrap(), ORIGINAL);
            }
        },
    );
}
