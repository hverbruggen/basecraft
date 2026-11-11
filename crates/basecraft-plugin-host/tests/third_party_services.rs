//! Host services are declared by any package, not granted by package identity.
use basecraft_plugin_host::registry::{self, Registry, RegistryConfig};
use basecraft_protocol::operation::{HostServices, QueryPreparation};
use serde_json::json;
use std::path::Path;

fn load(root: &Path, host: &HostServices) -> basecraft_plugin_host::Result<Registry> {
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
    let names = |names: &[&str]| names.iter().map(|&name| name.to_owned()).collect();
    let host = HostServices {
        species_workspace: true,
        bootstrap_exports: true,
        verify_inputs: names(&["reads"]),
        ambient_credentials: names(&["access"]),
        caller: Some("single_trace".into()),
        caller_engine: Some("example-caller".into()),
        query_preparation: Some(QueryPreparation::Blast),
        ..HostServices::default()
    };
    let temp = tempfile::tempdir().unwrap();
    let registry = load(temp.path(), &host).unwrap();
    let operation = registry
        .resolve("org.example.services/call@1", None)
        .unwrap();
    assert_eq!(operation.descriptor.host.as_ref(), Some(&host));

    // The same contract checks refuse invalid declarations on installation.
    let caller = HostServices {
        caller: Some("single_trace".into()),
        ..HostServices::default()
    };
    for invalid in [
        caller.clone(),
        HostServices {
            caller_engine: Some("Bad Name".into()),
            ..caller
        },
        HostServices {
            verify_inputs: names(&["missing"]),
            ..HostServices::default()
        },
        HostServices {
            ambient_credentials: names(&["undeclared"]),
            ..HostServices::default()
        },
    ] {
        let temp = tempfile::tempdir().unwrap();
        assert!(load(temp.path(), &invalid).is_err(), "{invalid:?}");
    }
}
