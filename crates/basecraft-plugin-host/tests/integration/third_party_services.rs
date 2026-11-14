//! Host services are declared by any package, not granted by package identity.
use basecraft_plugin_host::registry::{self, Registry, RegistryConfig};
use basecraft_protocol::operation::{
    CallerKind, HostServices, LifecyclePolicy, QueryPreparation, RemoteService,
};
use serde_json::json;
use std::path::Path;

fn load(root: &Path, host: &HostServices, engine: bool) -> basecraft_plugin_host::Result<Registry> {
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("plugin.toml"), "schema=2\ncontract='operations.json'\nid='org.example.services'\nname='Service fixture'\nversion='1.0.0'\n[entry]\noperation='python:plugin.py'\n").unwrap();
    std::fs::write(source.join("plugin.py"), "").unwrap();
    let contract = json!({"schema":2,"operations":[{"name":"call","version":"1.0.0","implementation":"call","title":"Call","description":"Third-party host service fixture","family":"file_analysis",
        "parameters":{"type":"object","additionalProperties":false,"properties":{}},
        "result":{"type":"object"},
        "inputs":{"reads":{"view":"bytes","media_type":"application/octet-stream","min":1,"max":1}},
        "outputs":{"artifacts":{"media_type":"application/vnd.basecraft.artifact-collection","min":1,"max":8,"visibility":"result","collection":true,"primary":"draft.json"}},
        "effects":{"document_write":false,"artifact_write":true},
        "needs":{"credentials":{"access":"BASECRAFT_EXAMPLE_TOKEN"}},"host":host}],
        "queries": if engine {json!([{"name":"engine","description":"The caller program","result":{"type":"object"}}])} else {json!([])}});
    std::fs::write(source.join("operations.json"), contract.to_string()).unwrap();
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
    let host = HostServices {
        species_workspace: true,
        caller: Some(CallerKind::SingleTrace),
        remote: None,
    };
    let temp = tempfile::tempdir().unwrap();
    let registry = load(temp.path(), &host, true).unwrap();
    let operation = registry
        .resolve("org.example.services/call@1", None)
        .unwrap();
    assert_eq!(operation.descriptor.host.as_ref(), Some(&host));

    // The same contract checks refuse invalid declarations on installation.
    // A caller answers the engine query.
    let temp = tempfile::tempdir().unwrap();
    assert!(load(temp.path(), &host, false).is_err());
    // A submission needs one receipt output and a query transform.
    let submission = HostServices {
        remote: Some(RemoteService {
            lifecycle: LifecyclePolicy::ExternalSubmission,
            query_preparation: Some(QueryPreparation::Blast),
            retained_query_transformation: None,
        }),
        ..HostServices::default()
    };
    let temp = tempfile::tempdir().unwrap();
    assert!(load(temp.path(), &submission, true).is_err());
}
