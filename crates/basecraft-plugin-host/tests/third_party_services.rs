//! Host services are declared by any package, not granted by package identity.
use basecraft_plugin_host::registry::{self, Registry, RegistryConfig};
use basecraft_protocol::operation::HostServices;
use serde_json::{Value, json};
use std::path::Path;

fn load(root: &Path, host: &Value) -> basecraft_plugin_host::Result<Registry> {
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("plugin.toml"), "schema=2\ncontract='operations.json'\nid='org.example.services'\nname='Service fixture'\nversion='1.0.0'\n[entry]\noperation='python:plugin.py'\n").unwrap();
    std::fs::write(source.join("plugin.py"), "").unwrap();
    let descriptor = json!({"name":"call","version":"1.0.0","implementation":"call","title":"Call","description":"Third-party host service fixture","family":"file_analysis",
        "parameters":{"type":"object","additionalProperties":false,"properties":{}},
        "result":{"type":"object"},
        "inputs":{"reads":{"view":"bytes","media_type":"application/octet-stream","min":1,"max":1}},
        "outputs":{"output":{"media_type":"application/json","min":1,"max":1,"visibility":"result"}},
        "effects":{"document_write":false,"artifact_write":true},
        "credentials":{"access":"BASECRAFT_EXAMPLE_TOKEN"},
        "required_features":["operation-contract-v1"],"host":host});
    std::fs::write(
        source.join("operations.json"),
        json!({"schema":1,"operations":[descriptor]}).to_string(),
    )
    .unwrap();
    let config = RegistryConfig {
        bundled: root.join("bundle"),
        packages: root.join("packages"),
        index: root.join("index.json"),
    };
    registry::install(&config, &source)?;
    Registry::load(&config)
}

#[test]
fn installed_packages_declare_the_services_built_in_plugins_use() {
    let host = json!({"species_workspace":true,"bootstrap_exports":true,"verify_inputs":["reads"],
        "ambient_credentials":["access"],"caller":"single_trace","caller_engine":"example-caller",
        "query_preparation":"blast"});
    let temp = tempfile::tempdir().unwrap();
    let registry = load(temp.path(), &host).unwrap();
    let operation = registry
        .resolve("org.example.services/call@1", None)
        .unwrap();
    let declared: HostServices = serde_json::from_value(host).unwrap();
    assert_eq!(operation.descriptor.host.as_ref(), Some(&declared));

    // The same contract checks refuse invalid declarations on installation.
    for invalid in [
        json!({"caller":"single_trace"}),
        json!({"caller":"single_trace","caller_engine":"Bad Name"}),
        json!({"verify_inputs":["missing"]}),
        json!({"ambient_credentials":["undeclared"]}),
    ] {
        let temp = tempfile::tempdir().unwrap();
        assert!(load(temp.path(), &invalid).is_err(), "{invalid}");
    }
}
