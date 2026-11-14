//! Publication ownership, collision and retry behavior.
use super::*;
use crate::registry::{Registry, RegistryConfig};
use basecraft_ops::records::{move_file as export_member, verify_export as verify_member_receipt};
use basecraft_platform::files::identity;
use basecraft_protocol::operation::{OperationPins, RuntimeState};
use serde_json::json;

fn bytes(value: &Value) -> Vec<u8> {
    basecraft_protocol::records::encode(value).unwrap()
}
fn member_oracle(destination: &Path, hash: &str) -> Value {
    let (device, inode) = identity(destination).unwrap();
    serde_json::to_value(
        basecraft_protocol::publication_records::MemberExportReceipt {
            schema: 1,
            destination: destination.into(),
            hash: hash.into(),
            device,
            inode,
            size: std::fs::metadata(destination).unwrap().len(),
        },
    )
    .unwrap()
}
fn registry(root: &Path) -> Registry {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin");
    let package = root.join("fixture");
    std::fs::create_dir(&package).unwrap();
    for name in ["plugin.toml", "operations.json"] {
        std::fs::copy(source.join(name), package.join(name)).unwrap();
    }
    let script = std::fs::read_to_string(source.join("plugin.py")).unwrap();
    // Exercise optional tool/process evidence without invoking any external tool.
    let script = script.replace(
        "return {\"data\": data, \"artifacts\": [{\"role\": \"report\", \"path\": \".\"}]}",
        "return {'data': data, 'artifacts': [{'role':'report','path':'.'}], **({'tool': {'name':'fixture','version':'1'}, 'execution': {'argv':['fixture','count'], 'resolved_parameters':{'z': {'z': 1, 'a': 2}, 'a': 0}, 'preprocessing':[], 'environment':{}, 'methods_template':None}} if mode == 'normal' else {})}",
    );
    std::fs::write(package.join("plugin.py"), script).unwrap();
    let config = RegistryConfig {
        bundled: root.join("absent"),
        packages: root.join("packages"),
        index: root.join("plugins.json"),
    };
    crate::registry::install(&config, &package).unwrap();
    Registry::load(&config).unwrap()
}

fn publication_setup(
    root: &Path,
    visible: bool,
    member: bool,
) -> (
    PreparedOperation,
    InvokeResult,
    Vec<(String, PathBuf, String)>,
    OperationOutcome,
) {
    std::fs::write(root.join("input.txt"), b"fixture input\n").unwrap();
    let registry = registry(root);
    let request: OperationRequest = serde_json::from_value(json!({
        "operation":"org.example.registry-fixture/count@1",
        "inputs":{"sources":[{"kind":"file","path":"input.txt"}]},
        "parameters":{},"destinations":{}
    }))
    .unwrap();
    let mut operation = registry.resolve(&request.operation, None).unwrap().clone();
    let binding = operation.descriptor.outputs.get_mut("report").unwrap();
    binding.collection = true;
    binding.max = 8;
    let mut prepared = PreparedOperation::prepare(
        operation,
        request,
        SessionPolicy::new(root, true).unwrap(),
        "publication fixture".into(),
        None,
        &registry,
        &[],
    )
    .unwrap();
    if visible {
        prepared
            .request
            .destinations
            .insert("report".into(), "visible λ collection".into());
    }
    if member {
        prepared
            .request
            .destinations
            .insert("report/a/file.txt".into(), "member λ.txt".into());
    }
    let staging = &prepared.run_path(&prepared.context.outputs["report"]);
    let names = ["a/file.txt", "a0.txt", "λ space/quote\".txt"];
    for (index, name) in names.iter().enumerate() {
        let path = staging.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("synthetic {index}\n")).unwrap();
    }
    let result: InvokeResult = serde_json::from_value(json!({"data":{"bytes":3},
        "artifacts":names.iter().map(|name|json!({"role":"report","path":name})).collect::<Vec<_>>()
    }))
    .unwrap();
    let validated = prepared.validate_result(&result).unwrap();
    let outcome = OperationOutcome {
        schema: 1,
        operation: prepared.operation.id.clone(),
        run_id: prepared.run.id().into(),
        state: RuntimeState::Finalizing,
        pins: OperationPins {
            generation: "fixture".into(),
            package_digest: prepared.operation.package.digest.clone(),
            descriptor_digest: prepared.operation.descriptor_digest.clone(),
        },
        requested_parameters: prepared.request.parameters.clone(),
        effective_parameters: prepared.effective_parameters(),
        data: None,
        artifacts: vec![],
        proposals: None,
        warnings: vec![],
        errors: vec![],
        evidence: None,
    };
    (prepared, result, validated, outcome)
}

fn publication_expected(
    prepared: &PreparedOperation,
    validated: &[(String, PathBuf, String)],
) -> BTreeMap<PathBuf, String> {
    let staging = &prepared.run_path(&prepared.context.outputs["report"]);
    validated
        .iter()
        .map(|(_, path, hash)| {
            (
                path.strip_prefix(staging).unwrap().to_path_buf(),
                hash.clone(),
            )
        })
        .collect()
}

#[test]
fn publication_collection_presence_only_finalize_gates() {
    for visible in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (prepared, result, validated, mut outcome) =
            publication_setup(temp.path(), visible, false);
        let expected = publication_expected(&prepared, &validated);
        prepared
            .finalize(&result, &validated, &mut outcome, false)
            .unwrap();
        let intent = prepared
            .run
            .artifact(if visible {
                "collection-report-export.json"
            } else {
                "collection-report-intent.json"
            })
            .unwrap();
        for raw in [
            b"".as_slice(),
            b"{malformed",
            b"{\"schema\":99,\"destination\":\"wrong\",\"members\":{},\"future\":true}",
        ] {
            std::fs::write(&intent, raw).unwrap();
            prepared
                .finalize(&result, &validated, &mut outcome, true)
                .unwrap();
            assert_eq!(std::fs::read(&intent).unwrap(), raw);
            assert!(
                prepared
                    .finalize(&result, &validated, &mut outcome, false)
                    .is_err()
            );
        }
        // Presence follows a regular symlink as before; no hardening in typing.
        #[cfg(unix)]
        {
            let target = temp.path().join("presence-target");
            std::fs::write(&target, b"not JSON").unwrap();
            std::fs::remove_file(&intent).unwrap();
            std::os::unix::fs::symlink(&target, &intent).unwrap();
            prepared
                .finalize(&result, &validated, &mut outcome, true)
                .unwrap();
            std::fs::remove_file(&intent).unwrap();
        }
        #[cfg(not(unix))]
        std::fs::remove_file(&intent).unwrap();
        assert!(
            prepared
                .finalize(&result, &validated, &mut outcome, true)
                .is_err()
        );
        std::fs::write(&intent, b"presence only").unwrap();
        let collection = if visible {
            temp.path().join("visible λ collection")
        } else {
            prepared.run.artifact("report").unwrap()
        };
        let path = collection.join("a/file.txt");
        let original = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        assert!(
            prepared
                .finalize(&result, &validated, &mut outcome, true)
                .is_err()
        );
        std::fs::write(&path, original).unwrap();
        let extra = collection.join("extra");
        std::fs::write(&extra, b"extra").unwrap();
        assert!(
            prepared
                .finalize(&result, &validated, &mut outcome, true)
                .is_err()
        );
        std::fs::remove_file(&extra).unwrap();
        let alias = temp.path().join("alias");
        std::fs::hard_link(&path, &alias).unwrap();
        assert!(
            prepared
                .finalize(&result, &validated, &mut outcome, true)
                .is_err()
        );
        std::fs::remove_file(alias).unwrap();
        #[cfg(unix)]
        {
            std::fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(temp.path().join("input.txt"), &path).unwrap();
            assert!(
                prepared
                    .finalize(&result, &validated, &mut outcome, true)
                    .is_err()
            );
            std::fs::remove_file(&path).unwrap();
            std::fs::write(&path, b"synthetic 0\n").unwrap();
        }
        verify_collection(&collection, &expected).unwrap();
    }
}

#[test]
fn publication_host_and_empty_collection_no_intent() {
    for host in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut prepared, result, validated, mut outcome) =
            publication_setup(temp.path(), false, false);
        if host {
            // Actual retained==staging branch; test-prepared host descriptor,
            // not a claim of end-to-end host workflow execution.
            prepared.operation.descriptor.host_workflow = Some("fixture".into());
            prepared
                .finalize(&result, &validated, &mut outcome, false)
                .unwrap();
        } else {
            prepared
                .finalize(&result, &[], &mut outcome, false)
                .unwrap();
        }
        assert!(
            !prepared
                .run
                .artifact("collection-report-intent.json")
                .unwrap()
                .exists()
        );
        assert!(
            !prepared
                .run
                .artifact("collection-report-export.json")
                .unwrap()
                .exists()
        );
    }
}

#[test]
fn publication_member_selected_reader_and_foreign_identity() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("source");
    let destination = root.join("destination");
    let receipt = root.join("receipt.json");
    std::fs::write(&source, b"owned").unwrap();
    let hash = basecraft_io::hash_bytes(b"owned");
    export_member(&source, &destination, &hash, &receipt).unwrap();
    let original = member_oracle(&destination, &hash);
    let mut extended = original.clone();
    extended["future"] = json!({"nested":[null,{}]});
    records::write_json(&receipt, &extended).unwrap();
    verify_member_receipt(&destination, &hash, &receipt).unwrap();
    for key in ["schema", "destination", "hash", "device", "inode", "size"] {
        for replacement in [
            None,
            Some(Value::Null),
            Some(json!(-1)),
            Some(json!("wrong")),
            Some(json!(1.5)),
        ] {
            let mut bad = original.clone();
            if let Some(value) = replacement {
                bad[key] = value;
            } else {
                bad.as_object_mut().unwrap().remove(key);
            }
            records::write_json(&receipt, &bad).unwrap();
            assert!(
                verify_member_receipt(&destination, &hash, &receipt).is_err(),
                "{key}: {bad}"
            );
        }
    }
    std::fs::write(&receipt, b"{malformed").unwrap();
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
    records::write_json(&receipt, &original).unwrap();
    let alias = root.join("alias");
    std::fs::hard_link(&destination, &alias).unwrap();
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
    std::fs::remove_file(&alias).unwrap();
    std::fs::write(&destination, b"other").unwrap();
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
    std::fs::write(&destination, b"owned").unwrap();
    verify_member_receipt(&destination, &hash, &receipt).unwrap();
    #[cfg(unix)]
    {
        std::fs::rename(&destination, &alias).unwrap();
        std::os::unix::fs::symlink(&alias, &destination).unwrap();
        assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
        std::fs::remove_file(&destination).unwrap();
        std::fs::rename(&alias, &destination).unwrap();
    }
    let foreign = root.join("foreign");
    std::fs::write(&foreign, b"owned").unwrap();
    assert_ne!(identity(&foreign).unwrap(), identity(&destination).unwrap());
    std::fs::rename(foreign, &destination).unwrap();
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
    std::fs::remove_file(&receipt).unwrap();
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
    std::fs::File::create(&receipt)
        .unwrap()
        .set_len((64 << 20) + 1)
        .unwrap();
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
    std::fs::remove_file(&receipt).unwrap();
    std::fs::create_dir(&receipt).unwrap();
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
}

#[test]
fn publication_failures_receipt_before_promotion_and_retry() {
    for visible in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (prepared, result, validated, mut outcome) =
            publication_setup(temp.path(), visible, false);
        let intent = prepared
            .run
            .artifact(if visible {
                "collection-report-export.json"
            } else {
                "collection-report-intent.json"
            })
            .unwrap();
        std::fs::create_dir(&intent).unwrap();
        assert!(
            prepared
                .finalize(&result, &validated, &mut outcome, false)
                .is_err()
        );
        // A collection bound for the project never sits in the run (D26).
        assert!(!prepared.run.artifact("report").unwrap().is_dir());
        assert!(!temp.path().join("visible λ collection").exists());
        std::fs::remove_dir(&intent).unwrap();
        std::fs::write(&intent, b"stale intent").unwrap();
        prepared
            .finalize(&result, &validated, &mut outcome, true)
            .unwrap();
        assert_ne!(std::fs::read(intent).unwrap(), b"stale intent");
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("source");
    let destination = root.join("destination");
    let receipt = root.join("receipt");
    std::fs::write(&source, b"source").unwrap();
    let hash = basecraft_io::hash_bytes(b"source");
    std::fs::create_dir(&receipt).unwrap();
    assert!(export_member(&source, &destination, &hash, &receipt).is_err());
    assert!(!destination.exists());
    std::fs::remove_dir(&receipt).unwrap();
    std::fs::write(&destination, b"foreign").unwrap();
    assert!(export_member(&source, &destination, &hash, &receipt).is_err());
    assert_eq!(std::fs::read(&destination).unwrap(), b"foreign");
    assert!(receipt.is_file());
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
    assert!(!std::fs::read_dir(root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".basecraft-export-")
    }));
    std::fs::remove_file(&destination).unwrap();
    std::fs::write(&receipt, b"stale receipt").unwrap();
    export_member(&source, &destination, &hash, &receipt).unwrap();
    assert!(!source.exists(), "the run keeps no copy (D26)");
    verify_member_receipt(&destination, &hash, &receipt).unwrap();
    assert_eq!(
        std::fs::read(&receipt).unwrap(),
        bytes(&member_oracle(&destination, &hash))
    );
}

#[cfg(unix)]
#[test]
fn publication_invalid_native_paths_return_json_at_producer_boundary() {
    use std::os::unix::ffi::OsStringExt;
    let invalid = std::ffi::OsString::from_vec(b"invalid-\xff".to_vec());
    // EX11: synthetic validated tuple, since normal plugin member names come
    // from JSON strings. Exercise actual finalize with a native map key.
    let temp = tempfile::tempdir().unwrap();
    let (prepared, result, _, mut outcome) = publication_setup(temp.path(), false, false);
    let staging = prepared.run_path(&prepared.context.outputs["report"]);
    std::fs::remove_dir_all(&staging).unwrap();
    std::fs::create_dir(&staging).unwrap();
    let path = staging.join(&invalid);
    std::fs::write(&path, b"native").unwrap();
    let hash = basecraft_io::hash_bytes(b"native");
    let intent = prepared
        .run
        .artifact("collection-report-intent.json")
        .unwrap();
    std::fs::write(&intent, b"old intent").unwrap();
    let error = prepared
        .finalize(
            &result,
            &[("report".into(), path.clone(), hash.clone())],
            &mut outcome,
            false,
        )
        .unwrap_err();
    assert!(matches!(error, Error::Json(_)), "{error}");
    assert_eq!(std::fs::read(intent).unwrap(), b"old intent");
    assert_eq!(std::fs::read(&path).unwrap(), b"native");
    assert!(!prepared.run.artifact("report").unwrap().exists());
    // EX12: test-prepared policy root changed after ordinary preparation. This
    // proves the finalizer boundary, not full-route invalid-root reachability.
    let temp = tempfile::tempdir().unwrap();
    let (mut prepared, result, validated, mut outcome) =
        publication_setup(temp.path(), true, false);
    let native_root = temp.path().join(&invalid);
    std::fs::create_dir(&native_root).unwrap();
    prepared.policy.root = native_root.clone();
    let intent = prepared
        .run
        .artifact("collection-report-export.json")
        .unwrap();
    std::fs::write(&intent, b"old export intent").unwrap();
    let error = prepared
        .finalize(&result, &validated, &mut outcome, false)
        .unwrap_err();
    assert!(matches!(error, Error::Json(_)), "{error}");
    // Nothing moved: the collection is still where the tool wrote it.
    assert!(!prepared.run.artifact("report").unwrap().exists());
    assert!(validated.iter().all(|(_, path, _)| path.is_file()));
    assert_eq!(outcome.artifacts, []);
    assert_eq!(std::fs::read(intent).unwrap(), b"old export intent");
    assert_eq!(std::fs::read_dir(native_root).unwrap().count(), 0);
    // EX13: direct existing helper boundary after copy/hash/sync/metadata.
    let source = temp.path().join("source");
    std::fs::write(&source, b"native").unwrap();
    let destination = temp.path().join(&invalid).join(&invalid);
    let receipt = temp.path().join("receipt");
    std::fs::write(&receipt, b"old receipt").unwrap();
    let error = export_member(&source, &destination, &hash, &receipt).unwrap_err();
    assert!(matches!(error, basecraft_ops::Error::Json(_)), "{error}");
    assert_eq!(std::fs::read(&receipt).unwrap(), b"old receipt");
    assert_eq!(std::fs::read(&source).unwrap(), b"native");
    assert!(!destination.exists());
    assert_eq!(
        std::fs::read_dir(destination.parent().unwrap())
            .unwrap()
            .count(),
        0
    );
    // verify_member_receipt's separate json!(destination) remains unchanged.
}
