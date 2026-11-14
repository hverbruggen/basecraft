//! Same-profile cooperating processes, ordered by socket barriers, never sleeps.
#![cfg(unix)]
use crate::decided_save;
use basecraft_ops::{
    Session,
    records::ArtifactRun,
    records::{self, SavePolicy},
};
use basecraft_protocol::Op;
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::Child,
    time::Duration,
};

const ORIGINAL: &[u8] = b">gene\nACGT\n";
struct Barrier(BufReader<UnixStream>);
impl Barrier {
    fn new(stream: UnixStream) -> Self {
        stream
            .set_read_timeout(Some(Duration::from_secs(40)))
            .unwrap();
        Self(BufReader::new(stream))
    }
    fn send(&mut self, value: &str) {
        writeln!(self.0.get_mut(), "{value}").unwrap();
    }
    fn expect(&mut self, expected: &str) {
        let mut line = String::new();
        self.0.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), expected);
    }
}
// Timeouts diagnose deadlocks; a failed real save establishes exclusion. Reap our children on failure so a broken guard cannot hang Cargo.
struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
impl Worker {
    /// Waits for the worker and checks that its test ran and passed.
    fn finish(&mut self) {
        let mut stdout = Vec::new();
        let mut pipe = self.0.stdout.take().unwrap();
        std::io::Read::read_to_end(&mut pipe, &mut stdout).unwrap();
        let status = self.0.wait().unwrap();
        let output = std::process::Output {
            status,
            stdout,
            stderr: Vec::new(),
        };
        crate::isolated_profile::assert_one_passed("publication_worker", &output);
    }
}
fn spawn(root: &Path, role: &str, mode: &str) -> (Worker, Barrier) {
    let socket = root.join(format!("{role}.sock"));
    let listener = UnixListener::bind(&socket).unwrap();
    let child =
        crate::isolated_profile::relaunch(&crate::isolated_profile::sibling("publication_worker"))
            .env("BASECRAFT_PUBLICATION_ROLE", role)
            .env("BASECRAFT_PUBLICATION_MODE", mode)
            .env("BASECRAFT_PUBLICATION_ROOT", root)
            .env("BASECRAFT_PROFILE_DIR", root.join("profile"))
            .env("BASECRAFT_STATE_DIR", root.join("profile/state"))
            .env("BASECRAFT_CACHE_DIR", root.join("profile/cache"))
            .env("TMPDIR", root)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
    (Worker(child), Barrier::new(listener.accept().unwrap().0))
}
fn scenario(mode: &str) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("input.fasta"), ORIGINAL).unwrap();
    let (mut registrar, mut registration) = spawn(root.path(), "registrar", mode);
    registration.expect("prepared");
    let (mut saver, mut saving) = spawn(root.path(), "saver", mode);
    saving.expect(if mode == "save-first" {
        "saved"
    } else {
        "locked"
    });
    registration.send("commit");
    registration.expect(if mode == "save-first" {
        "refused"
    } else {
        "published"
    });
    if mode != "save-first" {
        saving.send("resume");
        saving.expect("saved");
    }
    registrar.finish();
    saver.finish();
    assert_eq!(
        std::fs::read(root.path().join("input.fasta")).unwrap(),
        b">gene\nATGT\n"
    );
}
#[test]
fn registration_first_preserves_complete_owned_dependency_inventory() {
    scenario("preserve");
}
#[test]
fn registration_first_exposes_proposal_exemption_consistently() {
    scenario("proposal");
}
#[test]
fn interrupted_publication_keeps_original_guard_and_releases_lock() {
    scenario("interrupted");
}
#[test]
fn save_first_refuses_frozen_obsolete_original() {
    scenario("save-first");
}

#[test]
fn publication_worker() {
    let Ok(role) = std::env::var("BASECRAFT_PUBLICATION_ROLE") else {
        return;
    };
    let mode = std::env::var("BASECRAFT_PUBLICATION_MODE").unwrap();
    let root = PathBuf::from(std::env::var_os("BASECRAFT_PUBLICATION_ROOT").unwrap());
    let source = root.join("input.fasta");
    let mut barrier = Barrier::new(UnixStream::connect(root.join(format!("{role}.sock"))).unwrap());
    if mode.starts_with("retain-") {
        retention_worker(&root, &role, &mode, &mut barrier);
        return;
    }
    if role == "registrar" {
        let expected = basecraft_io::hash_file(&source).unwrap();
        // Freeze before publication, as runner preparation does.
        std::fs::copy(&source, root.join("frozen.fasta")).unwrap();
        if mode == "save-first" {
            barrier.send("prepared");
            barrier.expect("commit");
            let _locks = records::lock().unwrap();
            assert!(
                records::reference_input(
                    &root.join("result.txt"),
                    &source,
                    &expected,
                    None,
                    &[],
                    "stale registration",
                    None
                )
                .is_err()
            );
            assert_eq!(std::fs::read(root.join("frozen.fasta")).unwrap(), ORIGINAL);
            assert!(records::conflicts(&source).unwrap().analyses.is_empty());
            barrier.send("refused");
            return;
        }
        let locks = records::lock().unwrap();
        let owner = ArtifactRun::create(None, &root.join("result.txt"), "owner").unwrap();
        let ledger = records::reference_input(
            &root.join("result.txt"),
            &source,
            &expected,
            None,
            &[],
            "input",
            None,
        )
        .unwrap();
        // Pause at the former post-helper gap. A save must not acquire the lock
        // and preserve this ledger only for the stale record below to erase it.
        let mut input = records::read_input(ledger.directory()).unwrap();
        barrier.send("prepared");
        barrier.expect("commit");
        if mode == "interrupted" {
            // A real failed write after the reference publication leaves that
            // conservative reference protecting the source. There is no rollback.
            assert!(records::write_input(&source, &input).is_err());
            std::fs::write(
                root.join("ledger-path"),
                ledger.directory().as_os_str().as_encoded_bytes(),
            )
            .unwrap();
            drop(locks);
            barrier.send("published");
            return;
        }
        input.analysis_run =
            Some(records::relative(ledger.directory(), owner.directory()).unwrap());
        if mode == "proposal" {
            input.proposal_target = Some(true);
        }
        records::write_input(ledger.directory(), &input).unwrap();
        input.source.as_mut().unwrap().path =
            records::relative(owner.directory(), &source).unwrap();
        input.extra.insert("upstream".into(), json!([{
            "bundle":records::relative(owner.directory(), ledger.directory()).unwrap(),
            "id":ledger.id(), "hash":basecraft_io::hash_file(&ledger.artifact("manifest.json").unwrap()).unwrap()
        }]));
        records::write_input(owner.directory(), &input).unwrap();
        records::write_json(
            &owner.artifact("run.json").unwrap(),
            &json!({"result":"result.txt"}),
        )
        .unwrap();
        records::register(&owner).unwrap();
        std::fs::write(
            root.join("ledger-path"),
            ledger.directory().as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        drop(locks);
        barrier.send("published");
    } else {
        let mut session = Session::recoverable(&root).unwrap();
        let doc = session.open(&source).unwrap();
        session.apply(doc, Op::seq_replace(1, 2, "T")).unwrap();
        if mode != "save-first" {
            assert!(matches!(
                session.save(doc, None),
                Err(basecraft_ops::Error::Busy(_))
            ));
            barrier.send("locked");
            barrier.expect("resume");
        }
        if mode != "save-first" {
            let _locks = records::lock().unwrap();
            let conflicts = records::conflicts(&source).unwrap();
            if mode == "proposal" {
                assert!(conflicts.analyses.is_empty());
            } else {
                assert_eq!(conflicts.analyses.len(), 1);
                if mode != "interrupted" {
                    assert_eq!(conflicts.analyses[0].run.file_name().unwrap(), "owner");
                    assert_eq!(conflicts.analyses[0].backing_analyses.len(), 2);
                    let owner = records::read_input(&conflicts.analyses[0].run).unwrap();
                    assert_eq!(owner.upstream.len(), 1);
                }
            }
        }
        let recovered =
            basecraft_ops::records::recover(session.recovery_root(), &root.join("recovered"), None)
                .unwrap();
        assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">gene\nATGT\n");
        decided_save::save_with_policy(
            &mut session,
            doc,
            None,
            if mode == "preserve" || mode == "interrupted" {
                SavePolicy::PreserveCopy
            } else {
                SavePolicy::Cancel
            },
        )
        .unwrap();
        if mode != "save-first" {
            let ledger = PathBuf::from(std::fs::read_to_string(root.join("ledger-path")).unwrap());
            let input = records::read_input(&ledger).unwrap();
            assert_eq!(input.analysis_run.is_some(), mode != "interrupted");
            if mode == "proposal" {
                assert_eq!(input.proposal_target, Some(true));
                assert_eq!(
                    input.state,
                    basecraft_protocol::input_records::InputState::Referenced
                );
            } else {
                assert_eq!(
                    input.state,
                    basecraft_protocol::input_records::InputState::Preserved
                );
                // The mutable record must retain the preservation performed by
                // the save, rather than a later stale post-helper rewrite.
                assert_eq!(
                    std::fs::read(ledger.join("preserved-input")).unwrap(),
                    ORIGINAL
                );
            }
        }
        barrier.send("saved");
    }
}

// Exercise the same already-held preserve/read/delegate interval as retain_inputs.
// The real saver probes that interval in another process and then uses Session.
fn retention_scenario(mode: &str) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("input.fasta"), ORIGINAL).unwrap();
    let (mut retainer, mut retaining) = spawn(root.path(), "registrar", mode);
    retaining.expect("prepared");
    let (mut saver, mut saving) = spawn(root.path(), "saver", mode);
    let save_first = mode == "retain-break" || mode == "retain-remove";
    saving.expect(if save_first { "saved" } else { "locked" });
    retaining.send("commit");
    retaining.expect(if save_first { "refused" } else { "published" });
    if !save_first {
        saving.send("resume");
        saving.expect("saved");
        retaining.send("verify");
    }
    retainer.finish();
    saver.finish();
    assert_eq!(
        std::fs::read(root.path().join("input.fasta")).unwrap(),
        b">gene\nATGT\n"
    );
}

#[test]
fn retained_leaf_and_primary_exclude_preserving_save() {
    retention_scenario("retain-preserve");
}
#[test]
fn breaking_save_first_is_not_resurrected_by_retention() {
    retention_scenario("retain-break");
}
#[test]
fn removing_save_first_is_not_resurrected_by_retention() {
    retention_scenario("retain-remove");
}
#[test]
fn failed_retention_releases_guard_and_keeps_recoverable_leaf() {
    retention_scenario("retain-failure");
}

fn retention_worker(root: &Path, role: &str, mode: &str, barrier: &mut Barrier) {
    let source = root.join("input.fasta");
    if role == "registrar" {
        let hash = basecraft_io::hash_file(&source).unwrap();
        let locks = records::lock().unwrap();
        let owner = ArtifactRun::create(None, &root.join("result.txt"), "owner").unwrap();
        let leaf = records::reference_input(
            &root.join("result.txt"),
            &source,
            &hash,
            None,
            &[],
            "retained input",
            None,
        )
        .unwrap();
        let mut input = records::read_input(leaf.directory()).unwrap();
        input.analysis_run = Some(records::relative(leaf.directory(), owner.directory()).unwrap());
        records::write_input(leaf.directory(), &input).unwrap();
        input.extra.remove("analysis_run");
        input.source.as_mut().unwrap().path =
            records::relative(owner.directory(), &source).unwrap();
        input.extra.insert(
            "upstream".into(),
            json!([{
                "bundle":records::relative(owner.directory(), leaf.directory()).unwrap(),
                "id":leaf.id(),
                "hash":basecraft_io::hash_file(&leaf.artifact("manifest.json").unwrap()).unwrap()
            }]),
        );
        records::write_input(owner.directory(), &input).unwrap();
        records::write_json(
            &owner.artifact("run.json").unwrap(),
            &json!({"result":"result.txt"}),
        )
        .unwrap();
        records::register(&owner).unwrap();
        drop(locks);

        if mode == "retain-break" || mode == "retain-remove" {
            barrier.send("prepared");
            barrier.expect("commit");
            let _locks = records::lock().unwrap();
            assert!(records::preserve_input(leaf.directory()).is_err());
            if mode == "retain-break" {
                assert_eq!(
                    records::read_input(owner.directory()).unwrap().state,
                    basecraft_protocol::input_records::InputState::Broken
                );
                assert_eq!(
                    records::read_input(leaf.directory()).unwrap().state,
                    basecraft_protocol::input_records::InputState::Broken
                );
            } else {
                assert!(!owner.directory().exists());
                assert!(!leaf.directory().exists());
            }
            barrier.send("refused");
            return;
        }
        let locks = records::lock().unwrap();
        records::preserve_input(leaf.directory()).unwrap();
        records::preserve_input(leaf.directory()).unwrap();
        let mut primary = records::read_input(owner.directory()).unwrap();
        assert_eq!(
            primary.state,
            basecraft_protocol::input_records::InputState::Referenced
        );
        barrier.send("prepared");
        barrier.expect("commit");
        if mode == "retain-failure" {
            // An actual failed preserve and failed primary write release the guard
            // without erasing the successfully retained leaf or its provenance.
            assert!(records::preserve_input(&source).is_err());
            assert!(records::write_input(&source, &primary).is_err());
        } else {
            primary.origin_run =
                Some(records::relative(owner.directory(), leaf.directory()).unwrap());
            primary.state = basecraft_protocol::input_records::InputState::Preserved;
            records::write_input(owner.directory(), &primary).unwrap();
        }
        drop(locks);
        barrier.send("published");
        barrier.expect("verify");
        // A compatible public retry after save must validate the retained bytes
        // without recopying the changed live source.
        records::preserve_input(leaf.directory()).unwrap();
        assert_eq!(
            std::fs::read(leaf.directory().join("preserved-input")).unwrap(),
            ORIGINAL
        );
        let primary = records::read_input(owner.directory()).unwrap();
        assert_eq!(
            primary.state,
            basecraft_protocol::input_records::InputState::Preserved
        );
        if mode == "retain-preserve" {
            assert_eq!(
                primary.origin_run,
                Some(records::relative(owner.directory(), leaf.directory()).unwrap())
            );
            assert!(!owner.directory().join("preserved-input").exists());
        }
    } else {
        let mut session = Session::recoverable(root).unwrap();
        let doc = session.open(&source).unwrap();
        session.apply(doc, Op::seq_replace(1, 2, "T")).unwrap();
        let save_first = mode == "retain-break" || mode == "retain-remove";
        if !save_first {
            assert!(matches!(
                session.save(doc, None),
                Err(basecraft_ops::Error::Busy(_))
            ));
            barrier.send("locked");
            barrier.expect("resume");
            let _locks = records::lock().unwrap();
            if mode == "retain-preserve" {
                assert!(records::conflicts(&source).unwrap().analyses.is_empty());
            } else {
                assert_eq!(
                    records::read_input(
                        &ArtifactRun::runs_for_anchor(&root.join("result.txt"), 10)
                            .unwrap()
                            .into_iter()
                            .find(|path| path.file_name().unwrap() == "owner")
                            .unwrap()
                    )
                    .unwrap()
                    .state,
                    basecraft_protocol::input_records::InputState::Referenced
                );
            }
        }
        let recovered =
            basecraft_ops::records::recover(session.recovery_root(), &root.join("recovered"), None)
                .unwrap();
        assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">gene\nATGT\n");
        decided_save::save_with_policy(
            &mut session,
            doc,
            None,
            match mode {
                "retain-break" => SavePolicy::BreakEvidence,
                "retain-remove" => SavePolicy::RemoveAnalysis,
                _ => SavePolicy::PreserveCopy,
            },
        )
        .unwrap();
        barrier.send("saved");
    }
}
