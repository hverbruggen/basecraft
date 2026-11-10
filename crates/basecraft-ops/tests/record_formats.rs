//! Real writer regression checks. Keep the legacy builders as independent byte oracles.
use basecraft_ops::{Session, log, provenance};
use basecraft_platform::paths::AppPaths;
use basecraft_protocol::records::{PendingSave, RecoveryManifest, RecoveryStatus};
use basecraft_protocol::{Actor, Op};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

#[test]
fn recovery_and_pending_save_writer_bytes() {
    if std::env::var_os("BASECRAFT_RECORD_CHILD").is_none() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("profile");
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "recovery_and_pending_save_writer_bytes",
                "--nocapture",
            ])
            .env("BASECRAFT_RECORD_CHILD", root.path())
            .env("BASECRAFT_PROFILE_DIR", &profile)
            .env("XDG_STATE_HOME", &profile)
            .env("XDG_CACHE_HOME", &profile)
            .env("TMPDIR", root.path())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("BASECRAFT_RECORD_CHILD").unwrap());
    let file = root.join("gene.fasta");
    std::fs::write(&file, b">gene\nACGT\n").unwrap();
    let paths = AppPaths::discover().unwrap();
    let mut session = Session::recoverable_with_paths(&root, &paths).unwrap();
    session.set_actor(Actor::Script {
        client: "record-fixture".into(),
    });
    let doc = session.open(&file).unwrap();
    assert!(!session.recovery_root().exists());
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
    let baseline = log::read_all(session.recovery_root()).unwrap().remove(0);
    let expected = json!({
        "schema_version": 1, "session": session.id(), "source_root": root,
        "status": "unsaved", "baseline": baseline,
        "object_policy_limit": basecraft_ops::session::OBJECT_STORE_LIMIT
    });
    let bytes = std::fs::read(session.recovery_root().join("recovery.json")).unwrap();
    assert_eq!(bytes, serde_json::to_vec_pretty(&expected).unwrap());
    capture("recovery.json", &bytes);

    // Inject a publication failure after the main data write, retaining the intent.
    let bundle = provenance::companion(&file);
    std::fs::create_dir(&bundle).unwrap();
    std::fs::write(bundle.join("artifacts"), b"blocked").unwrap();
    let before = basecraft_io::hash_file(&file).unwrap();
    let hashes = session.save_hashes_at(doc, &file).unwrap();
    let error = session.save(doc, None).unwrap_err();
    assert!(error.to_string().contains("data saved"));
    let expected = json!({
        "schema_version": 1, "session": session.id(), "target": file,
        "before_hash": before, "expected_hashes": hashes,
        "status": "pending", "note": "data and provenance are separate writes"
    });
    let bytes = std::fs::read(session.recovery_root().join("pending-save.json")).unwrap();
    assert_eq!(bytes, serde_json::to_vec_pretty(&expected).unwrap());
    capture("pending-save.json", &bytes);
    assert_eq!(std::fs::read(&file).unwrap(), b">gene\nATGT\n");
    // Retry completes publication and removes the intent, with recovery still replayable.
    std::fs::remove_file(bundle.join("artifacts")).unwrap();
    session.save(doc, None).unwrap();
    assert!(!session.recovery_root().join("pending-save.json").exists());
    assert!(
        basecraft_ops::replay::replay(session.recovery_root())
            .unwrap()
            .ok()
    );

    let doc = session.create_sequence("new_gene", "ACGT").unwrap();
    let target = root.join("new-gene.gb");
    let bundle = provenance::companion(&target);
    std::fs::create_dir(&bundle).unwrap();
    std::fs::write(bundle.join("artifacts"), b"blocked").unwrap();
    let hashes = session.save_hashes_at(doc, &target).unwrap();
    assert!(session.save(doc, Some(&target)).is_err());
    let expected = json!({
        "schema_version": 1, "session": session.id(), "target": target,
        "before_hash": null, "expected_hashes": hashes,
        "status": "pending", "note": "data and provenance are separate writes"
    });
    let bytes = std::fs::read(session.recovery_root().join("pending-save.json")).unwrap();
    assert_eq!(bytes, serde_json::to_vec_pretty(&expected).unwrap());
    capture("pending-save-new.json", &bytes);
}

fn capture(name: &str, bytes: &[u8]) {
    if let Some(directory) = std::env::var_os("BASECRAFT_RECORD_CAPTURE") {
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}

fn sorted_bytes(value: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::to_value(value).unwrap()).unwrap()
}

#[test]
fn captured_records_round_trip_with_unknown_fields() {
    let bytes = include_bytes!("../../../testdata/fixtures/records-v1/recovery.json");
    let manifest: RecoveryManifest = serde_json::from_slice(bytes).unwrap();
    assert_eq!(sorted_bytes(&manifest), bytes);
    assert_eq!(manifest.status, RecoveryStatus::Unsaved);
    // Preserve future top-level fields, nested log fields and unknown status strings.
    // LogEntry alone would drop these nested fields (and reject a future op).
    let mut extended: Value = serde_json::from_slice(bytes).unwrap();
    extended["future"] = json!({"version": 9, "items": [null, 2, "value"]});
    extended["baseline"]["future_log_field"] = json!({"x": true});
    extended["baseline"]["op"] = json!("future.operation");
    extended["status"] = json!("future_status");
    let manifest: RecoveryManifest = serde_json::from_value(extended.clone()).unwrap();
    assert_eq!(
        manifest.status,
        RecoveryStatus::Unknown("future_status".into())
    );
    assert_eq!(sorted_bytes(&manifest), sorted_bytes(&extended));

    for bytes in [
        include_bytes!("../../../testdata/fixtures/records-v1/pending-save.json").as_slice(),
        include_bytes!("../../../testdata/fixtures/records-v1/pending-save-new.json").as_slice(),
    ] {
        let pending: PendingSave = serde_json::from_slice(bytes).unwrap();
        assert_eq!(pending.status, RecoveryStatus::Pending);
        assert_eq!(sorted_bytes(&pending), bytes);
        let mut extended: Value = serde_json::from_slice(bytes).unwrap();
        extended["future"] = json!({"nested": [1, false]});
        extended["status"] = json!("future_pending_state");
        let pending: PendingSave = serde_json::from_value(extended.clone()).unwrap();
        assert_eq!(sorted_bytes(&pending), sorted_bytes(&extended));
    }
}
