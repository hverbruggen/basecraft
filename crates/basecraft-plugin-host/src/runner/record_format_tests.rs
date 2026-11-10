//! Original-writer oracles. Keep literal shapes independent of the wire structs.
use super::*;
use crate::registry::{Registry, RegistryConfig};
use basecraft_protocol::operation::{OperationPins, RuntimeState};

fn bytes(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn capture(name: &str, path: &Path, expected: Value) {
    let actual = std::fs::read(path).unwrap();
    let original = bytes(&expected);
    assert_eq!(actual, original, "original writer oracle: {name}");
    if let Some(root) = std::env::var_os("BASECRAFT_RECORD_CAPTURE") {
        let root = PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(format!("{name}.json")), actual).unwrap();
        std::fs::write(root.join(format!("{name}.original.json")), original).unwrap();
    }
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

fn prepare(root: &Path, registry: &Registry, mode: &str) -> PreparedOperation {
    let request: OperationRequest = serde_json::from_value(json!({
        "operation":"org.example.registry-fixture/count@1",
        "inputs":{"sources":[{"kind":"file","path":"input.txt"}]},
        "parameters":{"mode":mode},"destinations":{"report":format!("{mode}.json")}
    }))
    .unwrap();
    let operation = registry.resolve(&request.operation, None).unwrap();
    PreparedOperation::prepare(
        operation,
        request,
        SessionPolicy::new(root, true).unwrap(),
        "record fixture".into(),
        None,
        registry,
    )
    .unwrap()
}

#[test]
fn original_execution_and_index_writers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("input.txt"), b"abc\n").unwrap();
    let registry = registry(root);
    for mode in ["normal", "delayed", "fail"] {
        let prepared = prepare(root, &registry, mode);
        let result = prepared.execute(Arc::new(AtomicBool::new(false)));
        assert_eq!(result.is_ok(), mode != "fail");
        let receipt = prepared.run.artifact("execution.json").unwrap();
        let recorded: Value = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
        // Output streams are process transport evidence; the oracle intentionally
        // takes those values from the file, but independently checks every key,
        // all invocation values, nulls, tool evidence and serialization bytes.
        capture(
            &format!("execution-{mode}"),
            &receipt,
            json!({
                "schema":1,"operation":prepared.operation.id,
                "package_version":prepared.operation.package.manifest.version,
                "package_digest":prepared.operation.package.digest,
                "descriptor_digest":prepared.operation.descriptor_digest,
                "runtime":prepared.operation.package.manifest.managed_runtime,
                "interpreter":prepared.operation.package.interpreter,
                "initiator":prepared.initiator,"executor":prepared.operation.package.manifest.id,
                "process_instance":basecraft_platform::process_instance(),"context":prepared.context,
                "requested_parameters":prepared.request.parameters,
                "effective_parameters":prepared.effective_parameters(),
                "stdout":recorded["stdout"],"stderr":recorded["stderr"],
                "tool":result.as_ref().ok().and_then(|r|r.tool.as_ref()),
                "execution":result.as_ref().ok().and_then(|r|r.execution.as_ref()),
            }),
        );
        assert_eq!(recorded["tool"].is_null(), mode != "normal");
        assert_eq!(recorded["execution"].is_null(), mode != "normal");
        if mode != "normal" {
            continue;
        }
        let states = [
            RuntimeState::Queued,
            RuntimeState::Running,
            RuntimeState::Cancelling,
            RuntimeState::Finalizing,
            RuntimeState::Completed,
            RuntimeState::Failed,
            RuntimeState::Cancelled,
            RuntimeState::Interrupted,
            RuntimeState::Partial,
        ];
        for state in states {
            let label = serde_json::to_value(state).unwrap();
            let label = label.as_str().unwrap();
            let outcome = OperationOutcome {
                schema: 1,
                operation: prepared.operation.id.clone(),
                run_id: prepared.run.id().into(),
                state,
                pins: OperationPins {
                    generation: "fixture".into(),
                    package_digest: prepared.operation.package.digest.clone(),
                    descriptor_digest: prepared.operation.descriptor_digest.clone(),
                },
                requested_parameters: prepared.request.parameters.clone(),
                effective_parameters: prepared.effective_parameters(),
                data: None,
                artifacts: vec![PublishedArtifact {
                    role: "report".into(),
                    path: "result.json".into(),
                    digest: "b3:fixture".into(),
                }],
                proposals: None,
                warnings: vec![],
                errors: if state == RuntimeState::Failed {
                    vec![
                        serde_json::from_value(
                            json!({"code":"tool_failure","message":"fixture failure",
                        "operation":prepared.operation.id}),
                        )
                        .unwrap(),
                    ]
                } else {
                    vec![]
                },
                evidence: Some(prepared.run.directory().display().to_string()),
            };
            prepared.persist(&outcome).unwrap();
            let path = basecraft_platform::paths::AppPaths::discover()
                .unwrap()
                .state
                .join("jobs")
                .join(prepared.run.id())
                .join("manifest.json");
            let actual: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(actual["ended"].is_null(), !state.terminal());
            if state.terminal() {
                assert!(actual["ended"].as_str().unwrap().ends_with('Z'));
            }
            let outputs: serde_json::Map<String, Value> = outcome
                .artifacts
                .iter()
                .map(|artifact| (artifact.path.clone(), json!(artifact.digest)))
                .collect();
            capture(
                &format!("job-index-{label}"),
                &path,
                json!({
                    "schema_version":1,"id":prepared.run.id(),"status":label,
                    "owner_instance":basecraft_platform::process_instance(),"started":prepared.started,
                    // No injected clock in production: validate the time shape above,
                    // then use the exact runtime value to compare the old serializer.
                    "ended":actual["ended"],
                    "plugin":{"id":prepared.operation.package.manifest.id,"version":prepared.operation.package.manifest.version},
                    "operation":prepared.operation.id,"effective_parameters":outcome.effective_parameters,
                    "initiator":prepared.initiator,
                    "source":{"path":root.join("input.txt").canonicalize().unwrap().display().to_string()},
                    "outputs":outputs,"diagnostics":outcome.errors.first().map(|error|error.message.clone()),
                    "runtime_record":prepared.run.artifact("runtime.json").unwrap(),
                }),
            );
        }
    }
    // A failed workflow writes the same distinct envelope as a successful one:
    // no result/tool/context fields. Invoke the real writer without scientific tools.
    let mut prepared = prepare(root, &registry, "normal");
    prepared.operation.package.manifest.id = "basecraft.host".into();
    prepared.operation.descriptor.host_workflow = Some("fixture-unknown".into());
    prepared.workflow_registry = Some(Arc::new(registry));
    let error = prepared
        .execute(Arc::new(AtomicBool::new(false)))
        .unwrap_err();
    assert_eq!(error.to_string(), "plugin protocol: unknown host workflow");
    capture(
        "execution-workflow",
        &prepared.run.artifact("execution.json").unwrap(),
        json!({
            "schema":1,"workflow":"fixture-unknown","initiator":prepared.initiator,
            "executor":"basecraft.host","requested_parameters":prepared.request.parameters,
            "effective_parameters":prepared.effective_parameters(),
            "package_digest":prepared.operation.package.digest,
            "descriptor_digest":prepared.operation.descriptor_digest,
            "process_instance":basecraft_platform::process_instance(),
        }),
    );
}
