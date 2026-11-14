//! M3 step 1: Session-level guards across store locations and file identities.
//! These pin existing protections, not the future registry/locking policy.
use crate::decided_save;
use std::path::Path;

use basecraft_ops::{
    DocId, Error, Session,
    records::ArtifactRun,
    records::{self, EvidenceConflict, SavePolicy},
};
use basecraft_protocol::Op;

const ORIGINAL: &[u8] = b">gene\nACGT\n";

// Reuse the budget/kind-conformance child-process pattern. Command::env selects
// the profile before any AppPaths/Session initialization; no process-global env
// mutation or serial lock is needed in the parallel parent harness.
use crate::isolated_profile;

fn isolated(test: impl FnOnce(&Path)) {
    if let Some(root) = isolated_profile::enter() {
        test(&root);
    }
}

fn folder(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn reference(root: &Path, source: &Path) -> ArtifactRun {
    records::reference_input(
        &root.join("result.txt"),
        source,
        &basecraft_io::hash_file(source).unwrap(),
        None,
        &[],
        "save protection regression",
        None,
    )
    .unwrap()
}

fn edited(root: &Path, file: &Path) -> (Session, DocId) {
    let mut session = Session::recoverable(root).unwrap();
    let doc = session.open(file).unwrap();
    session.apply(doc, Op::seq_replace(1, 2, "T")).unwrap();
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
    assert!(session.can_undo(doc).unwrap());
    let outputs = records::recover(session.recovery_root(), destination, None).unwrap();
    assert_eq!(outputs.len(), 1);
    assert!(
        std::fs::read_to_string(&outputs[0])
            .unwrap()
            .contains("ATGT")
    );
}

#[test]
fn registered_other_folder_policies_preserve_original_and_pending_edits() {
    isolated(|root| {
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
            let generation = session.generation(doc).unwrap();
            // The store is not an ancestor of the target: only registration
            // makes this conflict visible to Session.
            let found = conflict(session.save(doc, None).unwrap_err());
            assert_eq!(found.analyses.len(), 1);
            assert_eq!(found.analyses[0].run, run.directory());
            assert_eq!(session.generation(doc).unwrap(), generation);
            assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
            recoverable(&mut session, doc, &root.join(format!("recovered-{index}")));
            if policy == SavePolicy::Cancel {
                conflict(
                    decided_save::save_with_policy(&mut session, doc, None, policy).unwrap_err(),
                );
                assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
                assert_eq!(
                    records::read_input(run.directory()).unwrap().state,
                    basecraft_protocol::input_records::EvidenceState::Referenced
                );
                continue;
            }
            decided_save::save_with_policy(&mut session, doc, None, policy).unwrap();
            assert!(std::fs::read_to_string(&source).unwrap().contains("ATGT"));
            let input = records::read_input(run.directory()).unwrap();
            if policy == SavePolicy::PreserveCopy {
                let retained =
                    records::verified_source(run.directory(), input.source.as_ref().unwrap())
                        .unwrap();
                assert_eq!(std::fs::read(retained).unwrap(), ORIGINAL);
                assert_eq!(
                    input.state,
                    basecraft_protocol::input_records::EvidenceState::Preserved
                );
            } else {
                assert_eq!(
                    input.state,
                    basecraft_protocol::input_records::EvidenceState::Broken
                );
                assert!(
                    records::verified_source(run.directory(), input.source.as_ref().unwrap())
                        .is_err()
                );
            }
        }
    });
}

#[cfg(unix)]
#[test]
fn session_save_as_aliases_cannot_bypass_registered_input_guard() {
    isolated(|root| {
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
        decided_save::save_with_policy(&mut session, doc, Some(&hard), SavePolicy::PreserveCopy)
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
    });
}

#[test]
fn moved_project_store_is_discovered_with_stale_registry_location() {
    isolated(|root| {
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
        // The private index proves this directory moved; the known input
        // still blocks replacement and requires the normal preserve choice.
        let found = conflict(session.save(doc, None).unwrap_err());
        let check = session.check_save(doc, None).unwrap();
        assert!(check.evidence().is_complete());
        assert_eq!(found.analyses.len(), 1);
        assert_eq!(found.analyses[0].run, run);
        assert_eq!(std::fs::read(&source).unwrap(), ORIGINAL);
        recoverable(&mut session, doc, &root.join("recovered"));
        session
            .save_with_decision(doc, None, check.decide(SavePolicy::PreserveCopy, false))
            .unwrap();
        assert_eq!(
            std::fs::read(run.join("preserved-input")).unwrap(),
            ORIGINAL
        );
        assert!(std::fs::read_to_string(source).unwrap().contains("ATGT"));
    });
}

#[test]
fn lexical_old_path_still_guards_missing_or_replaced_source() {
    isolated(|root| {
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
                decided_save::save_with_policy(
                    &mut session,
                    doc,
                    Some(&source),
                    SavePolicy::PreserveCopy
                )
                .is_err()
            );
            assert_eq!(std::fs::read(&source).ok(), before);
            assert_eq!(std::fs::read(&moved).unwrap(), ORIGINAL);
            // Recovery to a new path is not blocked by the missing source.
            recoverable(&mut session, doc, &data.join("recovered"));
            decided_save::save_with_policy(
                &mut session,
                doc,
                Some(&source),
                SavePolicy::BreakEvidence,
            )
            .unwrap();
            assert_eq!(
                records::read_input(run.directory()).unwrap().state,
                basecraft_protocol::input_records::EvidenceState::Broken
            );
            assert!(std::fs::read_to_string(source).unwrap().contains("ATGT"));
            assert_eq!(std::fs::read(&moved).unwrap(), ORIGINAL);
        }
    });
}

#[test]
fn registered_transitive_dependencies_are_included_in_session_conflict() {
    isolated(|root| {
        let data = folder(root, "data");
        let elsewhere = folder(root, "other-project");
        let source = data.join("gene.fasta");
        std::fs::write(&source, ORIGINAL).unwrap();
        let parent = reference(&elsewhere, &source);
        let other = data.join("independent.fasta");
        std::fs::write(&other, b">other\nTTTT\n").unwrap();
        let dependent = |ancestor: &ArtifactRun| {
            records::reference_input(
                &elsewhere.join("dependent.txt"),
                &other,
                &basecraft_io::hash_file(&other).unwrap(),
                None,
                &[ancestor.directory().to_owned()],
                "dependent regression",
                None,
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
        decided_save::save_with_policy(&mut session, doc, None, SavePolicy::PreserveCopy).unwrap();
        assert_eq!(
            std::fs::read(parent.directory().join("preserved-input")).unwrap(),
            ORIGINAL
        );
        for run in [&child, &grandchild] {
            assert_eq!(
                records::read_input(run.directory()).unwrap().state,
                basecraft_protocol::input_records::EvidenceState::Referenced
            );
            let reference = records::read_input(parent.directory())
                .unwrap()
                .source
                .unwrap();
            let retained = records::verified_source(run.directory(), &reference).unwrap();
            assert_eq!(std::fs::read(retained).unwrap(), ORIGINAL);
        }
    });
}
