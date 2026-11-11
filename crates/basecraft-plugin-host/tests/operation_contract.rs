#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::path::Path;

use basecraft_plugin_host::{
    Manifest,
    contract::{self, BoundedSchema},
    process::verify_handshake,
};
use basecraft_protocol::plugin::PluginInfo;
use serde_json::json;

fn fixture() -> Manifest {
    Manifest::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin"),
    )
    .unwrap()
}

#[test]
fn schemas_and_defaults_are_strict_and_shared() {
    crate::isolated_profile::isolate!();
    let manifest = fixture();
    let mut descriptor = manifest.operation_contract().unwrap().operations.remove(0);
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
    crate::isolated_profile::isolate!();
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
    crate::isolated_profile::isolate!();
    let manifest = fixture();
    let contract = manifest.operation_contract().unwrap();
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
fn negotiation_refuses_another_protocol_or_identity() {
    crate::isolated_profile::isolate!();
    let manifest = fixture();
    let mut info = PluginInfo {
        protocol: 1,
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        operations: vec!["count".into()],
    };
    assert!(verify_handshake(&manifest, &info).is_err());
    info.protocol = basecraft_protocol::plugin::PROTOCOL_VERSION;
    verify_handshake(&manifest, &info).unwrap();
    info.operations.clear();
    assert!(verify_handshake(&manifest, &info).is_err());
    info.operations.push("count".into());
    info.id = "org.example.impostor".into();
    assert!(verify_handshake(&manifest, &info).is_err());
}

#[test]
fn the_sdk_runs_a_handler_on_paths_relative_to_the_run_folder() {
    use basecraft_plugin_host::PluginProcess;
    use basecraft_protocol::operation::InvocationContext;
    use std::collections::BTreeMap;
    crate::isolated_profile::isolate!();
    let manifest = fixture();
    let python = std::env::var_os("BASECRAFT_PYTHON").map_or_else(
        || std::path::PathBuf::from("python3"),
        std::path::PathBuf::from,
    );
    let mut process = PluginProcess::spawn_with(&manifest, &python).unwrap();
    process.ping().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let input = outside.path().join("input");
    std::fs::write(&input, b"ab\nc\n").unwrap();
    let context = InvocationContext {
        schema: basecraft_protocol::plugin::PROTOCOL_VERSION,
        operation: "org.example.registry-fixture/count@1".into(),
        run_id: "test".into(),
        run_root: temp.path().display().to_string(),
        inputs: BTreeMap::from([("sources".into(), vec![input.display().to_string()])]),
        outputs: BTreeMap::from([("report".into(), "report.json".into())]),
        cpu_threads: 1,
        cancel_file: "cancel".into(),
        progress_file: "progress".into(),
        input_evidence: BTreeMap::new(),
    };
    let result = process
        .invoke(
            "count",
            json!({"include_newlines":false,"mode":"normal"}),
            context.clone(),
            None,
        )
        .unwrap();
    assert_eq!(*result.data.unwrap(), json!({"bytes":3}));
    assert_eq!(result.artifacts[0].role, "report");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(temp.path().join("report.json")).unwrap()
        )
        .unwrap(),
        json!({"bytes":3})
    );
    assert!(process.invoke("missing", json!({}), context, None).is_err());
}

#[test]
fn every_bundled_plugin_declares_a_valid_schema_two_contract() {
    crate::isolated_profile::isolate!();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let mut checked = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if !path.join("plugin.toml").is_file() {
            continue;
        }
        let manifest = Manifest::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let contract = manifest.operation_contract().unwrap();
        assert_eq!(contract.schema, 2, "{}", path.display());
        for descriptor in &contract.operations {
            let declared = descriptor.outputs.values().filter(|o| o.primary.is_some());
            assert!(
                declared.count() <= 1,
                "{} declares two primary results",
                descriptor.name
            );
        }
        checked += 1;
    }
    assert!(checked >= 15);
}

/// Each bundled plugin starts, registers every declared implementation and
/// answers its declared queries with the declared shape. A plugin with its own
/// managed runtime is provisioned on install and is started by its own tests.
#[test]
fn every_bundled_plugin_starts_and_offers_what_it_declares() {
    crate::isolated_profile::isolate!();
    let python = basecraft_plugin_host::process::python_interpreter();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let mut started = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if !path.join("plugin.toml").is_file() {
            continue;
        }
        let manifest = Manifest::load(&path).unwrap();
        if manifest.runtime.is_some() {
            continue;
        }
        let mut process = basecraft_plugin_host::PluginProcess::spawn_with(&manifest, &python)
            .unwrap_or_else(|e| panic!("{}: {e}", manifest.id));
        for query in manifest.operation_contract().unwrap().queries {
            process
                .query(&query.name)
                .unwrap_or_else(|e| panic!("{} {}: {e}", manifest.id, query.name));
        }
        assert!(process.query("undeclared").is_err());
        process.finish().unwrap();
        started += 1;
    }
    assert!(started >= 13, "{started}");
}

#[test]
fn embedding_distinguishes_property_names_and_literal_data_from_schema_keywords() {
    crate::isolated_profile::isolate!();
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

#[test]
fn completed_data_schema_embeds_local_references_at_its_actual_envelope_location() {
    crate::isolated_profile::isolate!();
    let result = json!({"type":"object", "$defs":{"count":{"type":"integer","minimum":0}},
        "properties":{"bytes":{"$ref":"#/$defs/count"}},"required":["bytes"],"additionalProperties":false});
    let embedded = contract::embedded_schema(&result, "#/allOf/1/then/properties/data").unwrap();
    let envelope = json!({"allOf":[{"type":"object"},
        {"if":{"required":["state"],"properties":{"state":{"const":"completed"}}},
         "then":{"required":["data"],"properties":{"data":embedded}}}]});
    let schema = jsonschema::draft202012::new(&envelope).unwrap();
    assert!(schema.is_valid(&json!({"state":"queued","data":null})));
    assert!(schema.is_valid(&json!({"state":"completed","data":{"bytes":7}})));
    assert!(!schema.is_valid(&json!({"state":"completed","data":{"bytes":-1}})));
    assert!(!schema.is_valid(&json!({"state":"completed","data":null})));
}

#[test]
fn unique_diagnostics_do_not_square_unrelated_scientific_parameter_text() {
    crate::isolated_profile::isolate!();
    let schema = BoundedSchema::compile(&json!({"type":"object","properties":{
        "units":{"type":"string"},"diagnostics":{"type":"array","items":{"type":"string"},"uniqueItems":true}}})).unwrap();
    schema
        .validate(&json!({"units":"relative time from accepted aligned sequences; ".repeat(100)}))
        .unwrap();
    schema
        .validate(&json!({"units":"relative time","diagnostics":["one","two"]}))
        .unwrap();
    assert!(
        schema
            .validate(&json!({"diagnostics":["one","one"]}))
            .is_err()
    );
}
