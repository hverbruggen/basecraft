use std::path::Path;

use basecraft_plugin_host::{
    Manifest,
    policy::SessionPolicy,
    registry::{self, Registry, RegistryConfig},
};
use basecraft_protocol::operation::OperationRequest;
use serde_json::json;

fn config(root: &Path) -> RegistryConfig {
    RegistryConfig {
        bundled: root.join("missing-bundle"),
        packages: root.join("data/plugins"),
        index: root.join("config/plugins-v1.json"),
    }
}
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin")
}

#[test]
fn discovery_is_passive_and_installation_defaults_to_enabled() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    assert!(Registry::load(&config).unwrap().operations.is_empty());
    assert!(!config.index.exists());
    assert!(!config.packages.exists());
    let id = registry::install(&config, &fixture()).unwrap();
    let first = Registry::load(&config).unwrap();
    let second = Registry::load(&config).unwrap();
    assert_eq!(first.generation, second.generation);
    let name = "org.example.registry-fixture/count@1";
    let operation = first.resolve(name, None).unwrap();
    assert!(operation.package.enabled);
    let pinned_directory = operation.package.manifest.dir.clone();
    registry::set_enabled(&config, &id, false).unwrap();
    assert!(
        Registry::load(&config)
            .unwrap()
            .resolve(name, None)
            .is_err()
    );
    registry::install(&config, &fixture()).unwrap();
    assert!(!Registry::load(&config).unwrap().packages[&id].enabled);
    registry::set_enabled(&config, &id, true).unwrap();
    let enabled = Registry::load(&config).unwrap();
    enabled.resolve(name, None).unwrap();
    registry::remove(&config, &id).unwrap();
    assert!(Registry::load(&config).unwrap().operations.is_empty());
    // Existing jobs hold immutable metadata and package versions survive removal.
    assert!(pinned_directory.exists());
    operation.package.verify().unwrap();
    assert_eq!(
        first.resolve(name, None).unwrap().package.digest,
        operation.package.digest
    );
}

#[test]
fn updates_refresh_without_mutating_pins() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let id = registry::install(&config, &fixture()).unwrap();
    let first = Registry::load(&config).unwrap();
    let name = "org.example.registry-fixture/count@1";
    let old = first.resolve(name, None).unwrap();
    let pins = old.pins(&first.generation);
    registry::set_enabled(&config, &id, false).unwrap();
    let source = temp.path().join("updated");
    std::fs::create_dir(&source).unwrap();
    for file in ["plugin.toml", "operations.json", "plugin.py"] {
        std::fs::copy(fixture().join(file), source.join(file)).unwrap();
    }
    let text = std::fs::read_to_string(source.join("plugin.toml"))
        .unwrap()
        .replace("1.0.0", "1.1.0");
    std::fs::write(source.join("plugin.toml"), text).unwrap();
    registry::install(&config, &source).unwrap();
    let updated = Registry::load(&config).unwrap();
    assert!(!updated.packages[&id].enabled);
    registry::set_enabled(&config, &id, true).unwrap();
    let updated = Registry::load(&config).unwrap();
    assert_ne!(first.generation, updated.generation);
    assert!(updated.resolve(name, Some(&pins)).is_err());
    old.package.verify().unwrap();
    assert_ne!(
        old.package.manifest.dir,
        updated.operations[name].package.manifest.dir
    );
}

#[test]
fn editable_changes_duplicates_and_reserved_namespace_are_diagnostic() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let source = temp.path().join("editable");
    std::fs::create_dir(&source).unwrap();
    for file in ["plugin.toml", "operations.json", "plugin.py"] {
        std::fs::copy(fixture().join(file), source.join(file)).unwrap();
    }
    registry::register_editable(&config, &source).unwrap();
    let first = Registry::load(&config).unwrap();
    std::fs::write(
        source.join("plugin.py"),
        "raise RuntimeError('never imported by discovery')",
    )
    .unwrap();
    assert!(
        first
            .operations
            .values()
            .next()
            .unwrap()
            .package
            .verify()
            .is_err()
    );
    registry::install(&config, &source).unwrap();
    let duplicate = Registry::load(&config).unwrap();
    assert!(duplicate.operations.is_empty());
    assert_eq!(duplicate.diagnostics.len(), 2);
    let text = std::fs::read_to_string(source.join("plugin.toml"))
        .unwrap()
        .replace("org.example.registry-fixture", "basecraft.impostor");
    std::fs::write(source.join("plugin.toml"), text).unwrap();
    assert!(registry::install(&config, &source).is_err());
    let generation = Registry::reload(&config).unwrap();
    assert_eq!(generation.operations.len(), 1);
    assert_eq!(generation.diagnostics.len(), 1);
}

#[test]
fn disabled_and_read_only_sessions_fail_before_bindings_or_spawn() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let id = registry::install(&config, &fixture()).unwrap();
    let registry = Registry::load(&config).unwrap();
    let operation = registry.operations.values().next().unwrap();
    let request: OperationRequest = serde_json::from_value(json!({"operation":operation.id,"parameters":{},"inputs":{"sources":[{"kind":"file","path":"missing"}]}})).unwrap();
    let readonly = SessionPolicy::new(temp.path(), false).unwrap();
    assert!(!readonly.permits(operation));
    assert!(
        readonly
            .resolve(operation, &request)
            .unwrap_err()
            .to_string()
            .contains("read-only")
    );
    registry::set_enabled(&config, &id, false).unwrap();
    let registry = Registry::load(&config).unwrap();
    let writable = SessionPolicy::new(temp.path(), true).unwrap();
    assert!(!writable.permits(registry.operations.values().next().unwrap()));
}

#[test]
fn one_binding_resolver_checks_arrays_traversal_links_and_collisions() {
    use std::os::unix::fs::symlink;
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    registry::install(&config, &fixture()).unwrap();
    let registry = Registry::load(&config).unwrap();
    let operation = registry.operations.values().next().unwrap();
    std::fs::write(temp.path().join("source"), b"abc\n").unwrap();
    let policy = SessionPolicy::new(temp.path(), true).unwrap();
    let mut request: OperationRequest = serde_json::from_value(json!({"operation":operation.id,"parameters":{},"inputs":{"sources":[{"kind":"file","path":"source"}]},"destinations":{"report":"result.json"}})).unwrap();
    policy.resolve(operation, &request).unwrap();
    assert!(policy.path("../escape", false).is_err());
    symlink(temp.path().join("source"), temp.path().join("link")).unwrap();
    assert!(policy.path("link", false).is_err());
    symlink(temp.path(), temp.path().join("directory-link")).unwrap();
    assert!(policy.path("directory-link/out", true).is_err());
    std::fs::hard_link(temp.path().join("source"), temp.path().join("hardlink")).unwrap();
    assert!(policy.path("source", false).is_err());
    std::fs::remove_file(temp.path().join("hardlink")).unwrap();
    let duplicate = request.inputs["sources"][0].clone();
    request.inputs.get_mut("sources").unwrap().push(duplicate);
    assert!(policy.resolve(operation, &request).is_err());
    assert!(policy.path("source", true).is_err());
}

#[test]
fn schema_one_is_refused_and_the_contract_path_cannot_escape() {
    crate::isolated_profile::isolate!();
    let manifest = std::fs::read_to_string(fixture().join("plugin.toml")).unwrap();
    for text in [
        manifest.replace("schema = 2", "schema = 1"),
        manifest.replace("operations.json", "../operations.json"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("plugin.toml"), text).unwrap();
        assert!(Manifest::load(temp.path()).is_err());
    }
}

#[test]
fn the_registry_is_read_again_only_after_an_install_change() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let source = temp.path().join("editable");
    std::fs::create_dir(&source).unwrap();
    for file in ["plugin.toml", "operations.json", "plugin.py"] {
        std::fs::copy(fixture().join(file), source.join(file)).unwrap();
    }
    registry::register_editable(&config, &source).unwrap();
    let name = "org.example.registry-fixture/count@1";
    let title = |registry: Registry| registry.operations[name].descriptor.title.clone();
    let first = title(Registry::load(&config).unwrap());
    let mut contract: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("operations.json")).unwrap()).unwrap();
    contract["operations"][0]["title"] = "Count again".into();
    std::fs::write(source.join("operations.json"), contract.to_string()).unwrap();
    // No install change: the host does not read the registry again.
    assert_eq!(title(Registry::load(&config).unwrap()), first);
    assert_eq!(title(Registry::load(&config).unwrap()), first);
    // Disabling rewrites the index; the next load reads the registry once.
    registry::set_enabled(&config, "org.example.registry-fixture", false).unwrap();
    assert_eq!(title(Registry::load(&config).unwrap()), "Count again");
}
