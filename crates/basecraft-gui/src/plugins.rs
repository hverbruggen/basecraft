//! The window's view of the installed plugins. Plugin-host keeps one registry
//! per process and reads the plugin folders again only after an install,
//! update, removal or enable change rewrites the plugin index. Every lookup
//! here uses that copy; only a submission reads the package again, to verify
//! it before it runs.
use basecraft_plugin_host::{
    Manifest,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::operation::OperationDescriptor;
use serde_json::{Value, json};

pub(crate) fn registry() -> basecraft_plugin_host::Result<Registry> {
    Registry::load(&RegistryConfig::discover()?)
}

/// An operation's descriptor as registered.
pub(crate) fn descriptor(operation: &str) -> Result<OperationDescriptor, String> {
    registry()
        .map_err(|error| error.to_string())?
        .operations
        .get(operation)
        .map(|registered| registered.descriptor.clone())
        .ok_or_else(|| format!("unknown operation {operation}"))
}

/// The manifest of the bundled plugin in folder `name`, or why it did not load.
pub(crate) fn bundled_manifest(name: &str) -> Result<Manifest, String> {
    let dir = crate::bridge::plugin_dir(name);
    let registry = registry().map_err(|error| error.to_string())?;
    if let Some(package) = registry.packages.values().find(|p| p.manifest.dir == dir) {
        return Ok(package.manifest.clone());
    }
    Err(registry
        .diagnostics
        .into_iter()
        .find(|diagnostic| diagnostic.path == dir)
        .map_or_else(|| format!("plugin {name} is not installed"), |d| d.message))
}

/// Every registered operation with the settings form the picker can show, or
/// why it cannot run from the window.
pub(crate) fn catalog() -> basecraft_plugin_host::Result<Value> {
    use basecraft_protocol::operation::ExecutionFamily;
    let registry = registry()?;
    let mut rows = Vec::new();
    for operation in registry.operations.values() {
        let descriptor = &operation.descriptor;
        let properties = descriptor.parameters["properties"]
            .as_object()
            .ok_or_else(|| {
                basecraft_plugin_host::Error::Protocol("missing parameter properties".into())
            })?;
        let reason = if !operation.package.enabled {
            Some("Plugin is disabled")
        } else if operation.package.interpreter.is_none() {
            Some("Python runtime is unavailable")
        } else if descriptor.family != ExecutionFamily::FileAnalysis {
            Some("Use the sequence tools or CLI/Python/MCP for snapshot proposals")
        } else if descriptor
            .inputs
            .values()
            .any(|binding| binding.min > 1 || binding.max != 1)
        {
            Some("File lists are available through CLI/Python/MCP")
        } else if !basecraft_viewmodel::parameters::supported(&descriptor.parameters, 0) {
            Some("This settings schema is available through CLI/Python/MCP")
        } else {
            None
        };
        let fields: Vec<_> = properties.iter().map(|(name,schema)| json!({"name":name,"schema":schema,"required":descriptor.parameters["required"].as_array().is_some_and(|required| required.iter().any(|value| value == name))})).collect();
        rows.push(json!({"operation":operation.id,"title":descriptor.title,"description":descriptor.description,
            "parameters":descriptor.parameters,"supported":reason.is_none(),"limitation":reason,"fields":fields,"inputs":descriptor.inputs,"outputs":descriptor.outputs,"pins":operation.pins(&registry.generation)}));
    }
    Ok(json!({"operations":rows,"diagnostics":registry.diagnostics}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_plugin_host::registry::{register_editable, set_enabled};
    use std::path::Path;

    /// Runs the calling test again in a child process with a fresh profile, so
    /// installing a fixture plugin never touches the profile of the test run.
    /// Returns true inside the child.
    fn in_fresh_profile() -> bool {
        let name = std::thread::current()
            .name()
            .expect("test thread")
            .to_owned();
        if std::env::var("BASECRAFT_FRESH_PROFILE_CASE").as_deref() == Ok(&name) {
            return true;
        }
        let root = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env("BASECRAFT_FRESH_PROFILE_CASE", &name)
            .env("BASECRAFT_PROFILE_DIR", root.path().join("profile"))
            .env("TMPDIR", root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        false
    }

    /// The plugin folders are read once, and again once after each install
    /// change, however many lookups the window makes in between. Each lookup
    /// below follows an edit of the plugin's descriptor on disk; a lookup that
    /// read the folders would show that edit.
    #[test]
    fn lookups_read_the_plugin_folders_once_per_install_change() {
        if !in_fresh_profile() {
            return;
        }
        let config = RegistryConfig::discover().unwrap();
        let source = tempfile::tempdir().unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin");
        for file in ["plugin.toml", "operations.json", "plugin.py"] {
            std::fs::copy(fixture.join(file), source.path().join(file)).unwrap();
        }
        register_editable(&config, source.path()).unwrap();
        let operation = "org.example.registry-fixture/count@1";
        let edit = |title: &str| {
            let path = source.path().join("operations.json");
            let mut contract: Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            contract["operations"][0]["title"] = title.into();
            std::fs::write(&path, contract.to_string()).unwrap();
        };
        let listed = || {
            catalog().unwrap()["operations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["operation"] == operation)
                .unwrap()["title"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let mut seen = Vec::new();
        for lookup in 0..6 {
            edit(&format!("edit {lookup}"));
            seen.push(listed());
            seen.push(descriptor(operation).unwrap().title);
        }
        seen.dedup();
        assert_eq!(seen, ["edit 0"], "twelve lookups, one read");
        // Disabling rewrites the index: the next lookup reads once.
        set_enabled(&config, "org.example.registry-fixture", false).unwrap();
        edit("after the install change");
        assert_eq!(listed(), "after the install change");
        edit("not read");
        assert_eq!(
            descriptor(operation).unwrap().title,
            "after the install change"
        );
        assert!(bundled_manifest("orf-finder").is_ok());
        assert!(bundled_manifest("no-such-plugin").is_err());
    }
}
