use std::path::Path;

use basecraft_plugin_host::{
    Manifest,
    contract::{self, BoundedSchema},
    process::verify_handshake,
};
use basecraft_protocol::{operation::CONTRACT_CAPABILITY, plugin::PluginInfo};
use serde_json::json;

fn fixture() -> Manifest {
    Manifest::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin"),
    )
    .unwrap()
}

#[test]
fn schemas_and_defaults_are_strict_and_shared() {
    let manifest = fixture();
    let mut descriptor = manifest
        .operation_contract()
        .unwrap()
        .unwrap()
        .operations
        .remove(0);
    assert_eq!(
        contract::operation_id(&manifest.id, &descriptor).unwrap(),
        "org.example.registry-fixture/count@1"
    );
    assert_eq!(
        contract::normalize_parameters(&descriptor, &json!({})).unwrap(),
        json!({"mode":"normal", "include_newlines":true})
    );
    for bad in [
        json!({"mode":"unknown"}),
        json!({"include_newlines":"true"}),
        json!({"executable":"x"}),
        json!([]),
    ] {
        assert!(contract::normalize_parameters(&descriptor, &bad).is_err());
    }
    descriptor.parameters = json!({
        "type":"object", "additionalProperties":false,
        "$defs":{"n":{"type":"integer","minimum":1}},
        "properties":{"n":{"$ref":"#/$defs/n","default":2},"strict":{"type":"boolean"}},
        "if":{"properties":{"strict":{"const":true}},"required":["strict"]},
        "then":{"properties":{"n":{"minimum":4}}}
    });
    assert_eq!(
        contract::normalize_parameters(&descriptor, &json!({})).unwrap(),
        json!({"n":2})
    );
    assert!(contract::normalize_parameters(&descriptor, &json!({"strict":true})).is_err());
    let embedded = contract::embedded_parameters(&descriptor.parameters).unwrap();
    assert_eq!(
        embedded["properties"]["n"]["$ref"],
        "#/properties/parameters/$defs/n"
    );
    let outer = json!({"type":"object","properties":{"parameters":embedded}});
    let validator = jsonschema::draft202012::new(&outer).unwrap();
    for params in [
        json!({"n":1}),
        json!({"n":0}),
        json!({"n":3,"strict":true}),
        json!({"n":5,"strict":true}),
    ] {
        assert_eq!(
            validator.is_valid(&json!({"parameters":params})),
            BoundedSchema::compile(&descriptor.parameters)
                .unwrap()
                .validate(&params)
                .is_ok()
        );
    }
}

#[test]
fn rejects_remote_recursive_unknown_and_expensive_schemas() {
    for bad in [
        json!({"$ref":"https://example.invalid/schema"}),
        json!({"$ref":"file:///etc/passwd"}),
        json!({"$defs":{"x":{"$ref":"#/$defs/x"}},"$ref":"#/$defs/x"}),
        json!({"$vocabulary":{"https://example.invalid/vocab":true}}),
        json!({"pattern":"(x+)+"}),
    ] {
        assert!(BoundedSchema::compile(&bad).is_err());
    }
    let mut deep = json!(true);
    for _ in 0..40 {
        deep = json!({"items":deep});
    }
    assert!(BoundedSchema::compile(&deep).is_err());
    let schema = BoundedSchema::compile(&json!({"type":"array","uniqueItems":true})).unwrap();
    assert!(schema.validate(&json!(vec![0; 5000])).is_err());
}

#[test]
fn contract_identity_effects_and_defaults_are_checked() {
    let manifest = fixture();
    let contract = manifest.operation_contract().unwrap().unwrap();
    let mut bad = contract.clone();
    bad.operations.push(bad.operations[0].clone());
    assert!(contract::validate_contract(&manifest.id, &bad).is_err());
    for name in ["", "Bad", "with.dot", "x/y"] {
        let mut bad = contract.clone();
        bad.operations[0].name = name.into();
        assert!(contract::validate_contract(&manifest.id, &bad).is_err());
    }
    let mut bad = contract.clone();
    bad.operations[0].parameters["properties"]["mode"]["default"] = json!(42);
    assert!(contract::validate_contract(&manifest.id, &bad).is_err());
    let mut bad = contract.clone();
    bad.operations[0].effects.artifact_write = false;
    assert!(contract::validate_contract(&manifest.id, &bad).is_err());
}

#[test]
fn schema_two_negotiation_refuses_false_or_legacy_runtime() {
    let manifest = fixture();
    let mut info = PluginInfo {
        protocol: 1,
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        operations: vec!["count".into()],
        capabilities: vec![],
    };
    assert!(verify_handshake(&manifest, &info).is_err());
    info.capabilities.push(CONTRACT_CAPABILITY.into());
    verify_handshake(&manifest, &info).unwrap();
    info.operations.clear();
    assert!(verify_handshake(&manifest, &info).is_err());
    info.operations.push("count".into());
    info.id = "org.example.impostor".into();
    assert!(verify_handshake(&manifest, &info).is_err());
    let legacy: PluginInfo = serde_json::from_value(
        json!({"protocol":1,"id":"basecraft.orf-finder","version":"0.1.0","operations":[]}),
    )
    .unwrap();
    assert_eq!(legacy.capabilities, Vec::<String>::new());
}

#[test]
fn negotiated_sdk_context_runs_and_legacy_call_is_refused() {
    use basecraft_plugin_host::PluginProcess;
    use basecraft_protocol::operation::InvocationContext;
    use std::collections::BTreeMap;
    let manifest = fixture();
    let python = std::env::var_os("BASECRAFT_PYTHON").map_or_else(
        || std::path::PathBuf::from("python3"),
        std::path::PathBuf::from,
    );
    let mut process = PluginProcess::spawn_with(&manifest, &python).unwrap();
    process.ping().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    std::fs::write(&input, b"ab\nc\n").unwrap();
    let report = temp.path().join("report.json");
    let context = InvocationContext {
        schema: 1,
        operation: "org.example.registry-fixture/count@1".into(),
        run_id: "test".into(),
        inputs: BTreeMap::from([("sources".into(), vec![input.display().to_string()])]),
        outputs: BTreeMap::from([("report".into(), report.display().to_string())]),
        cpu_threads: 1,
        cancel_file: temp.path().join("cancel").display().to_string(),
        progress_file: temp.path().join("progress").display().to_string(),
    };
    let result = process
        .invoke_context(
            "count",
            json!({"include_newlines":false,"mode":"normal"}),
            context,
            None,
        )
        .unwrap();
    assert_eq!(*result.data.unwrap(), json!({"bytes":3}));
    assert_eq!(result.artifacts[0].role, "report");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(report).unwrap()).unwrap(),
        json!({"bytes":3})
    );
    assert!(process.invoke("count", json!({}), None).is_err());
}

#[test]
fn embedding_distinguishes_property_names_and_literal_data_from_schema_keywords() {
    let schema = json!({
        "type":"object", "$defs":{"text":{"type":"string"}},
        "properties":{
            "default":{"$ref":"#/$defs/text"},
            "$ref":{"type":"object","const":{"$ref":"a literal"}}
        }
    });
    let embedded = contract::embedded_parameters(&schema).unwrap();
    assert_eq!(
        embedded["properties"]["default"]["$ref"],
        "#/properties/parameters/$defs/text"
    );
    assert_eq!(embedded["properties"]["$ref"]["const"]["$ref"], "a literal");
}
