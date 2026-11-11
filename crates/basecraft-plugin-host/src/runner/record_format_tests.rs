//! Original-writer oracles. Keep literal shapes independent of the wire structs.
use super::*;
use crate::registry::{Registry, RegistryConfig};
use basecraft_protocol::operation::{OperationPins, RuntimeState};

fn bytes(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn capture(name: &str, path: &Path, expected: &Value) {
    let actual = std::fs::read(path).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let old_path = temp.path().join("original.json");
    input_evidence::write_json(&old_path, expected).unwrap();
    let original = std::fs::read(old_path).unwrap();
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
    let operation = registry.resolve(&request.operation, None).unwrap().clone();
    PreparedOperation::prepare(
        operation,
        request,
        SessionPolicy::new(root, true).unwrap(),
        "record fixture".into(),
        None,
        registry,
        &[],
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
            &json!({
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
                &json!({
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
    Arc::make_mut(&mut prepared.operation.package).manifest.id = "basecraft.host".into();
    prepared.operation.descriptor.host_workflow = Some("fixture-unknown".into());
    prepared.workflow_registry = Some(Arc::new(registry));
    let error = prepared
        .execute(Arc::new(AtomicBool::new(false)))
        .unwrap_err();
    assert_eq!(error.to_string(), "plugin protocol: unknown host workflow");
    capture(
        "execution-workflow",
        &prepared.run.artifact("execution.json").unwrap(),
        &json!({
            "schema":1,"workflow":"fixture-unknown","initiator":prepared.initiator,
            "executor":"basecraft.host","requested_parameters":prepared.request.parameters,
            "effective_parameters":prepared.effective_parameters(),
            "package_digest":prepared.operation.package.digest,
            "descriptor_digest":prepared.operation.descriptor_digest,
            "process_instance":basecraft_platform::process_instance(),
        }),
    );
}

// Synthetic registered stages exercise workflow success without a scientific
// engine, remote service or changes to the workflow implementation.
#[test]
fn original_successful_workflow_writer() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("input.txt"), b">a\nACGT\n>b\nACGA\n").unwrap();
    let external_registry = registry(root);
    let mut prepared = prepare(root, &external_registry, "normal");
    let bundled = root.join("bundled");
    let package = bundled.join("stages");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("plugin.toml"),
        "schema = 2\nid = \"basecraft.delimitation\"\nname = \"Record fixture\"\nversion = \"1.0.0\"\ncontract = \"operations.json\"\n[entry]\noperation = \"python:fixture.py\"\n").unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/registry-plugin/operations.json");
    let mut contract: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let mut descriptor = contract["operations"][0].clone();
    descriptor["inputs"] = json!({"alignment":{"view":"bytes","media_type":"application/octet-stream","min":1,"max":1}});
    descriptor["outputs"] = json!({"artifacts":{"media_type":"application/json","min":1,"max":1,"visibility":"result","collection":true}});
    descriptor["parameters"] =
        json!({"type":"object","properties":{},"additionalProperties":false});
    descriptor["result"] = json!({"type":"object"});
    let mut operations = vec![];
    for name in ["haplotypes", "asap"] {
        descriptor["name"] = json!(name);
        descriptor["implementation"] = json!(name);
        operations.push(descriptor.clone());
    }
    contract["operations"] = json!(operations);
    std::fs::write(package.join("operations.json"), bytes(&contract)).unwrap();
    std::fs::write(package.join("fixture.py"), r"
import json
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin('basecraft.delimitation', '1.0.0')
def stage(snapshot, parameters, context):
    target = Path(context['outputs']['artifacts']) / 'result.json'
    target.write_text(json.dumps({'metrics': {'haplotypes': {'original_count': 2}}, 'workspace': None}) + '\n')
    return {'data': {}, 'artifacts': [{'role':'artifacts','path':'result.json'}]}
plugin.context_operation('haplotypes')(stage)
plugin.context_operation('asap')(stage)
plugin.run()
").unwrap();
    let stages = Registry::load(&RegistryConfig {
        bundled,
        packages: root.join("none"),
        index: root.join("none.json"),
    })
    .unwrap();
    Arc::make_mut(&mut prepared.operation.package).manifest.id = "basecraft.host".into();
    prepared.operation.descriptor.host_workflow = Some("asap_alignment".into());
    prepared.workflow_registry = Some(Arc::new(stages));
    prepared.parameters["unique_haplotypes"] = json!(false);
    prepared.context.inputs.insert(
        "alignment".into(),
        prepared.context.inputs["sources"].clone(),
    );
    prepared.context.input_evidence.insert(
        "alignment".into(),
        prepared.context.input_evidence["sources"].clone(),
    );
    let output = prepared.run.artifact("workflow").unwrap();
    std::fs::create_dir(&output).unwrap();
    prepared
        .context
        .outputs
        .insert("artifacts".into(), output.display().to_string());
    let result = prepared.execute(Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(result.artifacts[0].path, "fit.json");
    capture(
        "execution-workflow-success",
        &prepared.run.artifact("execution.json").unwrap(),
        &json!({
            "schema":1,"workflow":"asap_alignment","initiator":prepared.initiator,
            "executor":"basecraft.host","requested_parameters":prepared.request.parameters,
            "effective_parameters":prepared.effective_parameters(),
            "package_digest":prepared.operation.package.digest,
            "descriptor_digest":prepared.operation.descriptor_digest,
            "process_instance":basecraft_platform::process_instance(),
        }),
    );
}
fn roundtrip<T: serde::de::DeserializeOwned + serde::Serialize>(name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/records-v1")
        .join(name);
    let original = std::fs::read(path).unwrap();
    let record: T = serde_json::from_slice(&original).unwrap();
    assert_eq!(bytes(&serde_json::to_value(record).unwrap()), original);
    let mut extended: Value = serde_json::from_slice(&original).unwrap();
    extended["future"] = json!({"z":[null,{"z":1,"a":[false,2]}],"a":"extension"});
    for key in ["context", "tool", "execution", "plugin", "source"] {
        if extended[key].is_object() {
            extended[key]["future"] = json!({"z":[{"a":1}],"a":null});
        }
    }
    if extended["context"].is_object() {
        extended["context"]["input_evidence"]["sources"][0]["future"] = json!({"nested":[1,null]});
    }
    if extended["execution"].is_object() {
        extended["execution"]["methods"] =
            json!({"description":"fixture","citations":[],"future":{"x":[null,{}]}});
    }
    if extended.get("runtime").is_some() {
        extended["runtime"] = json!({"future":{"z":[1,null],"a":{}}});
    }
    let record: T = serde_json::from_value(extended.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), extended);
}

#[test]
fn typed_execution_and_index_examples_roundtrip() {
    use basecraft_protocol::records::{ExternalExecution, HostWorkflowExecution, LocalJobIndex};
    for mode in ["normal", "delayed", "fail"] {
        roundtrip::<ExternalExecution>(&format!("execution-{mode}.json"));
    }
    roundtrip::<HostWorkflowExecution>("execution-workflow.json");
    roundtrip::<HostWorkflowExecution>("execution-workflow-success.json");
    for state in [
        "queued",
        "running",
        "cancelling",
        "finalizing",
        "completed",
        "failed",
        "cancelled",
        "interrupted",
        "partial",
    ] {
        roundtrip::<LocalJobIndex>(&format!("job-index-{state}.json"));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/records-v1");
    let external = std::fs::read(root.join("execution-normal.json")).unwrap();
    let workflow = std::fs::read(root.join("execution-workflow.json")).unwrap();
    assert!(serde_json::from_slice::<ExternalExecution>(&workflow).is_err());
    assert!(serde_json::from_slice::<HostWorkflowExecution>(&external).is_err());
    // These are wire descriptions, not replacements for selected-field readers.
    // Missing required shape discriminators and malformed index states still fail.
    let mut index: Value =
        serde_json::from_slice(&std::fs::read(root.join("job-index-queued.json")).unwrap())
            .unwrap();
    index["status"] = json!({"malformed":true});
    assert!(serde_json::from_value::<LocalJobIndex>(index).is_err());
}

#[test]
fn original_prepared_run_writer() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("input.txt"), b"abc\n").unwrap();
    let registry = registry(root);
    let prepared = prepare(root, &registry, "normal");
    capture(
        "run-prepared",
        &prepared.run.artifact("run.json").unwrap(),
        &json!({"schema_version":1,"id":prepared.run.id(),"result":"normal.json","status":"prepared"}),
    );
    // No-export registered operations use their full operation ID as label.
    let mut request = prepared.request.clone();
    request.destinations.clear();
    let operation = registry.resolve(&request.operation, None).unwrap().clone();
    let prepared = PreparedOperation::prepare(
        operation,
        request,
        SessionPolicy::new(root, true).unwrap(),
        "record fixture".into(),
        None,
        &registry,
        &[],
    )
    .unwrap();
    capture(
        "run-prepared-no-export",
        &prepared.run.artifact("run.json").unwrap(),
        &json!({"schema_version":1,"id":prepared.run.id(),"result":"org.example.registry-fixture/count@1","status":"prepared"}),
    );
}

// Test-prepared registered collection: real validation/finalization, synthetic
// files only. No plugin process or scientific engine is executed.
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
    let staging = Path::new(&prepared.context.outputs["report"]);
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
    let staging = Path::new(&prepared.context.outputs["report"]);
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

fn member_oracle(destination: &Path, hash: &str) -> Value {
    // Independent actual promoted-file stat/hash, never receipt-derived values.
    assert_eq!(basecraft_io::hash_file(destination).unwrap(), hash);
    assert_eq!(link_count(destination).unwrap(), 1);
    json!({"schema":1,"destination":destination,"hash":hash,
        "device":identity(destination).unwrap().0,"inode":identity(destination).unwrap().1,
        "size":std::fs::metadata(destination).unwrap().len()})
}

#[test]
fn publication_original_producer_bytes() {
    let temp = if let Some(root) = std::env::var_os("BASECRAFT_PUBLICATION_FIXTURE_ROOT") {
        tempfile::tempdir_in(root).unwrap()
    } else {
        tempfile::tempdir().unwrap()
    };
    let root = temp.path();
    let (prepared, result, validated, mut outcome) = publication_setup(root, true, true);
    let expected = publication_expected(&prepared, &validated);
    prepared
        .finalize(&result, &validated, &mut outcome, false)
        .unwrap();
    capture(
        "collection-intent",
        &prepared
            .run
            .artifact("collection-report-intent.json")
            .unwrap(),
        &json!({"schema":1,"members":expected}),
    );
    capture(
        "collection-export",
        &prepared
            .run
            .artifact("collection-report-export.json")
            .unwrap(),
        &json!({"schema":1,"destination":root.join("visible λ collection"),"members":expected}),
    );
    let destination = root.join("member λ.txt");
    let receipt = prepared
        .run
        .artifact(&format!(
            "member-{}-export.json",
            basecraft_io::hash_bytes(destination.as_os_str().as_encoded_bytes())
                .trim_start_matches("b3:")
        ))
        .unwrap();
    capture(
        "member-export",
        &receipt,
        &member_oracle(&destination, &expected[Path::new("a/file.txt")]),
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"synthetic 0\n");
    let source = root.join("empty");
    std::fs::write(&source, []).unwrap();
    let destination = root.join("empty λ.txt");
    let receipt = root.join("empty-receipt.json");
    let hash = basecraft_io::hash_bytes(b"");
    export_member(&source, &destination, &hash, &receipt).unwrap();
    capture(
        "member-export-empty",
        &receipt,
        &member_oracle(&destination, &hash),
    );
}

// A deterministic observer/racer at the receipt-before-rename boundary. Test
// state is thread-local and consumed before invocation; no production policy.
type PromotionHook = Box<dyn FnOnce(&Path, &Path, &Path)>;
thread_local! {
    static PROMOTION_HOOK: std::cell::RefCell<Option<PromotionHook>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn member_promotion_hook(pending: &Path, destination: &Path, receipt: &Path) {
    let hook = PROMOTION_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook(pending, destination, receipt);
    }
}

#[test]
fn publication_known_shapes_roundtrip_and_ordering() {
    use basecraft_protocol::publication_records::{
        CollectionExportIntent, CollectionIntent, MemberExportReceipt,
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/records-v1");
    for name in [
        "collection-intent",
        "collection-export",
        "member-export",
        "member-export-empty",
    ] {
        let original = std::fs::read(root.join(format!("{name}.json"))).unwrap();
        let value = match name {
            "collection-intent" => {
                serde_json::to_value(serde_json::from_slice::<CollectionIntent>(&original).unwrap())
                    .unwrap()
            }
            "collection-export" => serde_json::to_value(
                serde_json::from_slice::<CollectionExportIntent>(&original).unwrap(),
            )
            .unwrap(),
            _ => serde_json::to_value(
                serde_json::from_slice::<MemberExportReceipt>(&original).unwrap(),
            )
            .unwrap(),
        };
        assert_eq!(bytes(&value), original);
        assert!(original.ends_with(b"\n") && !original.ends_with(b"\n\n"));
    }
    let members = BTreeMap::from([
        (PathBuf::from("a0.txt"), "zero".into()),
        (PathBuf::from("a/file.txt"), "nested".into()),
        (PathBuf::from("λ \\\".txt"), "unicode".into()),
    ]);
    let record = CollectionIntent {
        schema: 1,
        members: members.clone(),
    };
    assert_eq!(
        bytes(&serde_json::to_value(&record).unwrap()),
        bytes(&json!({"schema":1,"members":members}))
    );
    // Path component ordering and JSON string ordering differ on this pair.
    assert_ne!(
        serde_json::to_vec_pretty(&record).unwrap(),
        serde_json::to_vec_pretty(&serde_json::to_value(&record).unwrap()).unwrap()
    );
    let empty = CollectionIntent {
        schema: 1,
        members: BTreeMap::new(),
    };
    assert_eq!(
        bytes(&serde_json::to_value(empty).unwrap()),
        bytes(&json!({"schema":1,"members":{}}))
    );
    // Synthetic numeric extremes, not claimed to be emitted by this filesystem.
    let record = MemberExportReceipt {
        schema: 1,
        destination: PathBuf::from(r"C:\native\λ file.txt"),
        hash: "b3:synthetic".into(),
        device: u64::MAX,
        inode: (1_u64 << 53) + 1,
        size: u64::MAX - 1,
    };
    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(value["device"].as_u64(), Some(u64::MAX));
    assert_eq!(
        bytes(&value),
        bytes(
            &json!({"schema":1,"destination":record.destination,"hash":record.hash,"device":u64::MAX,"inode":(1_u64<<53)+1,"size":u64::MAX-1})
        )
    );
    for name in ["schema", "destination", "hash", "device", "inode", "size"] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(name);
        assert!(serde_json::from_value::<MemberExportReceipt>(missing).is_err());
    }
    assert!(
        serde_json::from_value::<CollectionExportIntent>(json!({"schema":1,"members":{}})).is_err()
    );
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
    input_evidence::write_json(&receipt, &extended).unwrap();
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
            input_evidence::write_json(&receipt, &bad).unwrap();
            assert!(
                verify_member_receipt(&destination, &hash, &receipt).is_err(),
                "{key}: {bad}"
            );
        }
    }
    std::fs::write(&receipt, b"{malformed").unwrap();
    assert!(verify_member_receipt(&destination, &hash, &receipt).is_err());
    input_evidence::write_json(&receipt, &original).unwrap();
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
        assert_eq!(prepared.run.artifact("report").unwrap().is_dir(), visible);
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
    assert!(export_member(&source, &destination, "wrong hash", &receipt).is_err());
    assert!(!receipt.exists() && !destination.exists());
    std::fs::create_dir(&receipt).unwrap();
    assert!(export_member(&source, &destination, &hash, &receipt).is_err());
    assert!(!destination.exists());
    std::fs::remove_dir(&receipt).unwrap();
    PROMOTION_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(|pending, destination, receipt| {
        assert!(!destination.exists());
        let hash = basecraft_io::hash_file(pending).unwrap();
        // Receipt has actual pending identity before any rename.
        assert_eq!(std::fs::read(receipt).unwrap(), bytes(&json!({"schema":1,"destination":destination,"hash":hash,
            "device":identity(pending).unwrap().0,"inode":identity(pending).unwrap().1,"size":std::fs::metadata(pending).unwrap().len()})));
        std::fs::write(destination, b"foreign").unwrap();
    })));
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
    let staging = PathBuf::from(&prepared.context.outputs["report"]);
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
    assert!(prepared.run.artifact("report").unwrap().is_dir());
    assert_eq!(outcome.artifacts.len(), validated.len());
    assert!(prepared.run.artifact("runtime.json").unwrap().is_file());
    assert_eq!(std::fs::read(intent).unwrap(), b"old export intent");
    assert_eq!(std::fs::read_dir(native_root).unwrap().count(), 0);
    // EX13: direct existing helper boundary after copy/hash/sync/metadata.
    let source = temp.path().join("source");
    std::fs::write(&source, b"native").unwrap();
    let destination = temp.path().join(&invalid).join(&invalid);
    let receipt = temp.path().join("receipt");
    std::fs::write(&receipt, b"old receipt").unwrap();
    let error = export_member(&source, &destination, &hash, &receipt).unwrap_err();
    assert!(matches!(error, Error::Json(_)), "{error}");
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
