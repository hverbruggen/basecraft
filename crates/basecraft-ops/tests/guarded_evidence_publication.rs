//! Same-profile cooperating processes, ordered by socket barriers, never sleeps.
#![cfg(unix)]
use basecraft_ops::{
    Session,
    analysis_store::ArtifactRun,
    input_evidence::{self, Mutation, SavePolicy},
};
use basecraft_protocol::Op;
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command},
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
fn spawn(root: &Path, role: &str, mode: &str) -> (Child, Barrier) {
    let socket = root.join(format!("{role}.sock"));
    let listener = UnixListener::bind(&socket).unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "publication_worker", "--nocapture"])
        .env("BASECRAFT_PUBLICATION_ROLE", role)
        .env("BASECRAFT_PUBLICATION_MODE", mode)
        .env("BASECRAFT_PUBLICATION_ROOT", root)
        .env("BASECRAFT_PROFILE_DIR", root.join("profile"))
        .env("BASECRAFT_STATE_DIR", root.join("profile/state"))
        .env("BASECRAFT_CACHE_DIR", root.join("profile/cache"))
        .env("BASECRAFT_SCRATCH_DIR", root.join("scratch"))
        .env("TMPDIR", root)
        .spawn()
        .unwrap();
    (child, Barrier::new(listener.accept().unwrap().0))
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
        saving.expect("saved");
    }
    assert!(registrar.wait().unwrap().success());
    assert!(saver.wait().unwrap().success());
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
    if role == "registrar" {
        let expected = basecraft_io::hash_file(&source).unwrap();
        // Freeze before publication, as runner preparation does.
        std::fs::copy(&source, root.join("frozen.fasta")).unwrap();
        if mode == "save-first" {
            barrier.send("prepared");
            barrier.expect("commit");
            let mutation = Mutation::begin().unwrap();
            assert!(
                mutation
                    .reference_input_with_dependencies(
                        &root.join("result.txt"),
                        &source,
                        &expected,
                        &[],
                        "stale registration"
                    )
                    .is_err()
            );
            assert_eq!(std::fs::read(root.join("frozen.fasta")).unwrap(), ORIGINAL);
            assert!(
                input_evidence::conflicts(&source)
                    .unwrap()
                    .analyses
                    .is_empty()
            );
            barrier.send("refused");
            return;
        }
        let mutation = Mutation::begin().unwrap();
        let owner = ArtifactRun::create(&root.join("result.txt"), "owner").unwrap();
        let ledger = mutation
            .reference_input_with_dependencies(
                &root.join("result.txt"),
                &source,
                &expected,
                &[],
                "input",
            )
            .unwrap();
        // Pause at the former post-helper gap. A save must not acquire the lock
        // and preserve this ledger only for the stale record below to erase it.
        let mut input = mutation.read_input(ledger.directory()).unwrap();
        barrier.send("prepared");
        barrier.expect("commit");
        input.extra.insert(
            "analysis_run".into(),
            json!(input_evidence::relative(ledger.directory(), owner.directory()).unwrap()),
        );
        if mode == "proposal" {
            input
                .extra
                .insert(input_evidence::PROPOSAL_TARGET.into(), json!(true));
        }
        mutation.write_input(ledger.directory(), &input).unwrap();
        input.source.as_mut().unwrap().path =
            input_evidence::relative(owner.directory(), &source).unwrap();
        input.extra.insert("upstream".into(), json!([{
            "bundle":input_evidence::relative(owner.directory(), ledger.directory()).unwrap(),
            "id":ledger.id(), "hash":basecraft_io::hash_file(&ledger.artifact("manifest.json").unwrap()).unwrap()
        }]));
        mutation.write_input(owner.directory(), &input).unwrap();
        input_evidence::write_json(
            &owner.artifact("run.json").unwrap(),
            &json!({"result":"result.txt"}),
        )
        .unwrap();
        mutation.register(&owner).unwrap();
        std::fs::write(
            root.join("ledger-path"),
            ledger.directory().as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        drop(mutation);
        barrier.send("published");
    } else {
        let mut session = Session::recoverable(&root).unwrap();
        let doc = session.open(&source).unwrap();
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
        session.flush().unwrap();
        let recovered =
            basecraft_ops::replay::recover(session.recovery_root(), &root.join("recovered"))
                .unwrap();
        assert_eq!(std::fs::read(&recovered[0]).unwrap(), b">gene\nATGT\n");
        if mode != "save-first" {
            let paths = basecraft_platform::paths::AppPaths::discover().unwrap();
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(paths.state.join("analysis-evidence.lock"))
                .unwrap();
            // A real second process probes the actual OS lock. No timeout or
            // absence-of-message is used as evidence of exclusion.
            assert!(fs4::fs_std::FileExt::try_lock_exclusive(&file).is_err());
            barrier.send("locked");
        }
        session
            .save_with_evidence(
                doc,
                None,
                if mode == "preserve" {
                    SavePolicy::PreserveCopy
                } else {
                    SavePolicy::Cancel
                },
            )
            .unwrap();
        if mode != "save-first" {
            let ledger = PathBuf::from(std::fs::read_to_string(root.join("ledger-path")).unwrap());
            let input = input_evidence::read_input(&ledger).unwrap();
            assert!(input.extra.contains_key("analysis_run"));
            if mode == "proposal" {
                assert_eq!(input.extra[input_evidence::PROPOSAL_TARGET], true);
                assert_eq!(input.state, "referenced");
            } else {
                assert_eq!(input.state, "preserved");
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
