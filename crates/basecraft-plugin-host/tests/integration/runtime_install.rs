//! Network-free installer regression checks. Real Pixi invocation is a separate
//! opt-in test, using a disposable external profile and an actual channel solve.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use basecraft_plugin_host::{
    Manifest, PluginProcess,
    registry::{self, Registry, RegistryConfig},
};

fn fixture() -> PathBuf {
    std::env::var_os("BASECRAFT_RUNTIME_TEST_FIXTURE").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin"),
        PathBuf::from,
    )
}
fn config(root: &Path) -> RegistryConfig {
    RegistryConfig {
        bundled: root.join("missing"),
        packages: root.join("data/plugins"),
        index: root.join("config/plugins-v1.json"),
    }
}
fn source(root: &Path, runtime: &str) -> PathBuf {
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    for file in ["plugin.toml", "operations.json", "plugin.py"] {
        std::fs::copy(fixture().join(file), source.join(file)).unwrap();
    }
    let path = source.join("plugin.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, format!("{text}\n{runtime}")).unwrap();
    source
}
fn executable(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
fn fake_pixi(root: &Path) -> PathBuf {
    let pixi = root.join("pixi");
    let python = basecraft_plugin_host::process::python_interpreter();
    // This tool is injected into this installation only. The selected Python
    // wrapper also exposes nativecheck so actual launch PATH can be observed.
    executable(
        &pixi,
        &format!(
            r#"#!/bin/sh
if [ "$1" = --version ]; then echo 'pixi controlled-test'; exit 0; fi
if /usr/bin/grep -q impossible pixi.toml; then echo 'solve failed: impossible' >&2; exit 9; fi
printf '%s\n' "$@" > pixi-args.txt
mkdir -p .pixi/envs/default/bin
printf '#!/bin/sh\nexec "{}" "$@"\n' > .pixi/envs/default/bin/python
printf '#!/bin/sh\necho managed-native\n' > .pixi/envs/default/bin/nativecheck
chmod +x .pixi/envs/default/bin/python .pixi/envs/default/bin/nativecheck
printf 'controlled lock\n' > pixi.lock
"#,
            python.display()
        ),
    );
    pixi
}
const DECLARED: &str =
    "[runtime]\nexecutables = [\"nativecheck\"]\n[runtime.dependencies]\nxz = \"*\"\n";

#[test]
fn runtime_is_frozen_updates_preserve_disabled_and_failed_solve_preserves_active() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let source = source(temp.path(), DECLARED);
    let pixi = fake_pixi(temp.path());
    let id = registry::install_with_pixi(&config, &source, &pixi).unwrap();
    let first = Registry::load(&config).unwrap();
    let package = &first.packages[&id];
    let runtime = package.manifest.managed_runtime.as_ref().unwrap();
    assert!(!runtime.root.starts_with(&package.manifest.dir));
    assert_eq!(package.interpreter.as_ref(), Some(&runtime.interpreter));
    std::fs::write(temp.path().join("input"), "abc").unwrap();
    let request = serde_json::from_value(serde_json::json!({"operation":"org.example.registry-fixture/count@1", "parameters":{},"inputs":{"sources":[{"kind":"file","path":"input"}]},"destinations":{"report":"report"}})).unwrap();
    let accepted = basecraft_plugin_host::runner::PreparedOperation::prepare(
        first
            .resolve("org.example.registry-fixture/count@1", None)
            .unwrap()
            .clone(),
        request,
        basecraft_plugin_host::policy::SessionPolicy::new(temp.path(), true).unwrap(),
        "runtime update test".into(),
        None,
        &first,
        &[],
    )
    .unwrap();
    registry::set_enabled(&config, &id, false).unwrap();
    registry::install_with_pixi(&config, &source, &pixi).unwrap();
    let updated = Registry::load(&config).unwrap();
    assert!(!updated.packages[&id].enabled);
    assert_ne!(first.generation, updated.generation);
    assert_ne!(
        runtime.root,
        updated.packages[&id]
            .manifest
            .managed_runtime
            .as_ref()
            .unwrap()
            .root
    );
    package.verify().unwrap();
    let result = accepted
        .execute(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )))
        .unwrap();
    assert_eq!(result.data, Some(Box::new(serde_json::json!({"bytes":3}))));
    let evidence: serde_json::Value = serde_json::from_slice(
        &std::fs::read(accepted.run.artifact("execution.json").unwrap()).unwrap(),
    )
    .unwrap();
    // The run record holds the environment: the lock, the conda
    // specification it was solved from and the interpreter's version.
    let environment = &evidence["environment"];
    assert_eq!(
        environment["runtime"]["lock_digest"],
        runtime.lock_digest.as_str()
    );
    assert_eq!(
        environment["runtime"]["dependencies"],
        serde_json::to_value(&package.manifest.runtime.as_ref().unwrap().dependencies).unwrap()
    );
    assert!(
        environment["interpreter"]
            .as_str()
            .unwrap()
            .starts_with("Python 3."),
        "{environment}"
    );
    assert!(
        !evidence
            .to_string()
            .contains(runtime.root.to_str().unwrap())
    );
    let mut process =
        PluginProcess::spawn_with(&package.manifest, Path::new("/does/not/exist")).unwrap();
    process.finish().unwrap();
    let mut command = std::process::Command::new(&runtime.interpreter);
    basecraft_plugin_host::runtime::configure_command(
        &mut command,
        &package.manifest,
        &runtime.interpreter,
    )
    .unwrap();
    let output = command
        .args([
            "-c",
            "import subprocess; print(subprocess.check_output(['nativecheck'], text=True).strip())",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "managed-native"
    );
    let before = std::fs::read(&config.index).unwrap();
    let text = std::fs::read_to_string(source.join("plugin.toml"))
        .unwrap()
        .replace("xz = \"*\"", "impossible = \"*\"");
    std::fs::write(source.join("plugin.toml"), text).unwrap();
    let error = registry::install_with_pixi(&config, &source, &pixi)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("installation") && error.contains("solve failed"),
        "{error}"
    );
    assert_eq!(before, std::fs::read(&config.index).unwrap());
    package.verify().unwrap();
    let current_runtime = updated.packages[&id]
        .manifest
        .managed_runtime
        .as_ref()
        .unwrap();
    std::fs::remove_file(
        current_runtime
            .root
            .join(".pixi/envs/default/bin/nativecheck"),
    )
    .unwrap();
    // Damage outside the host shows on an explicit refresh.
    let missing = Registry::reload(&config).unwrap();
    assert!(missing.packages.is_empty());
    assert!(missing.diagnostics[0].message.contains("nativecheck"));
    assert!(missing.diagnostics[0].message.contains("not executable"));
    // Retained lock corruption gives a useful discovery/invocation diagnosis.
    std::fs::write(runtime.root.join("pixi.lock"), "changed").unwrap();
    assert!(
        package
            .verify()
            .unwrap_err()
            .to_string()
            .contains("runtime")
    );
}

#[test]
fn missing_broken_and_unexecutable_pixi_fail_without_activation() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    registry::install(&config, &fixture()).unwrap();
    let before = std::fs::read(&config.index).unwrap();
    let source = source(temp.path(), DECLARED);
    let missing = temp.path().join("missing-pixi");
    let error = registry::install_with_pixi(&config, &source, &missing)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("provisioner unavailable") && error.contains("missing-pixi"),
        "{error}"
    );
    let broken = temp.path().join("broken-pixi");
    executable(&broken, "#!/bin/sh\necho broken executable >&2\nexit 23\n");
    let error = registry::install_with_pixi(&config, &source, &broken)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("provisioner unavailable") && error.contains("broken executable"),
        "{error}"
    );
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o644)).unwrap();
    let error = registry::install_with_pixi(&config, &source, &broken)
        .unwrap_err()
        .to_string();
    assert!(error.contains("provisioner unavailable"), "{error}");
    assert_eq!(before, std::fs::read(&config.index).unwrap());
}

#[test]
fn missing_declared_runtime_is_diagnostic_discovery_never_provisions() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let source = source(temp.path(), DECLARED);
    registry::register_editable(&config, &source).unwrap();
    let before = std::fs::read(&config.index).unwrap();
    let loaded = Registry::load(&config).unwrap();
    assert!(loaded.packages.is_empty());
    assert!(
        loaded.diagnostics[0]
            .message
            .contains("runtime unavailable")
    );
    assert_eq!(before, std::fs::read(&config.index).unwrap());
    assert!(!temp.path().join("data").exists());
    assert!(
        PluginProcess::spawn(&Manifest::load(&source).unwrap())
            .err()
            .unwrap()
            .to_string()
            .contains("runtime unavailable")
    );
}

#[test]
fn missing_required_native_command_cannot_be_satisfied_by_ambient_path() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let source = source(temp.path(), &DECLARED.replace("nativecheck", "sh"));
    let error = registry::install_with_pixi(&config, &source, &fake_pixi(temp.path()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing runtime executables"), "{error}");
    assert!(!config.index.exists());
}

#[test]
#[ignore = "networked Pixi install; run explicitly in an external disposable profile"]
fn actual_pixi_installs_and_invokes_declared_runtime() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let source = source(
        temp.path(),
        "[runtime]\nexecutables = [\"xz\"]\n[runtime.dependencies]\nxz = \"5.*\"\n[runtime.pypi-dependencies]\npackaging = \">=24\"\n",
    );
    let id = registry::install(&config, &source).unwrap();
    let registry = Registry::load(&config).unwrap();
    let package = &registry.packages[&id];
    let mut process =
        PluginProcess::spawn_with(&package.manifest, Path::new("/missing-ambient-python")).unwrap();
    std::fs::write(temp.path().join("input"), "abc").unwrap();
    let context = serde_json::from_value(serde_json::json!({
        "schema":2,"operation":"org.example.registry-fixture/count@1","run_id":"test-runtime",
        "run_root":temp.path(),"inputs":{"sources":["input"]},"outputs":{"report":"report"},
        "cpu_threads":1,"cancel_file":"cancel","progress_file":"progress"
    }))
    .unwrap();
    let result = process
        .invoke(
            "count",
            serde_json::json!({"mode":"normal", "include_newlines":true}),
            context,
            None,
        )
        .unwrap();
    assert_eq!(result.data, Some(Box::new(serde_json::json!({"bytes":3}))));
    process.finish().unwrap();
    let runtime = package.manifest.managed_runtime.as_ref().unwrap();
    let mut command = std::process::Command::new(&runtime.interpreter);
    basecraft_plugin_host::runtime::configure_command(
        &mut command,
        &package.manifest,
        &runtime.interpreter,
    )
    .unwrap();
    let output = command.args(["-c", "import sys, packaging, subprocess; print(sys.executable); print(subprocess.check_output(['xz','--version'],text=True))"]).output().unwrap();
    assert!(output.status.success());
    println!(
        "runtime receipt: {}",
        serde_json::to_string_pretty(runtime).unwrap()
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
    println!("retained disposable profile: {}", temp.keep().display());
}

#[test]
fn bundled_frozen_runtime_retains_old_selection_and_rejects_changed_lock_or_version() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let mut config = config(temp.path());
    let runtime = DECLARED.replace("[runtime]\n", "[runtime]\nlock = \"runtime.lock\"\n")
        + "\n[runtime.version-checks.nativecheck]\nargs = [\"--version\"]\nexpected = \"managed-native\"\n";
    let source = source(temp.path(), &runtime);
    config.bundled = temp.path().to_owned();
    std::fs::write(source.join("runtime.lock"), "controlled lock\n").unwrap();
    let id = Manifest::load(&source).unwrap().id;
    let before = Registry::load(&config).unwrap();
    assert!(!before.packages.contains_key(&id));
    assert!(!config.index.exists());
    let pixi = fake_pixi(temp.path());
    registry::provision_bundled_with_pixi(&config, &id, &pixi).unwrap();
    let first = Registry::load(&config).unwrap();
    let package = first.packages[&id].clone();
    let selected = package.manifest.managed_runtime.as_ref().unwrap();
    assert_eq!(selected.reported_versions["nativecheck"], "managed-native");
    assert!(selected.executable_digests.contains_key("nativecheck"));
    let mut command = std::process::Command::new(&selected.interpreter);
    command.env(
        "BASECRAFT_RUNTIME_EXECUTABLES",
        "inherited-untrusted-selection",
    );
    basecraft_plugin_host::runtime::configure_command(
        &mut command,
        &package.manifest,
        &selected.interpreter,
    )
    .unwrap();
    let output = command
        .args(["-c", "import os,json; print(json.loads(os.environ['BASECRAFT_RUNTIME_EXECUTABLES'])['nativecheck'])"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        selected
            .root
            .join(".pixi/envs/default/bin/nativecheck")
            .to_str()
            .unwrap()
    );
    assert!(
        std::fs::read_to_string(selected.root.join("pixi-args.txt"))
            .unwrap()
            .lines()
            .any(|arg| arg == "--locked")
    );
    // Reopening an already provisioned bundle does not need Pixi/network.
    registry::provision_bundled_with_pixi(&config, &id, Path::new("/missing-pixi")).unwrap();
    let index = std::fs::read(&config.index).unwrap();
    let manifest = std::fs::read_to_string(source.join("plugin.toml")).unwrap();
    std::fs::write(
        source.join("plugin.toml"),
        manifest.replace("managed-native", "wrong-version"),
    )
    .unwrap();
    let error = registry::provision_bundled_with_pixi(&config, &id, &pixi)
        .unwrap_err()
        .to_string();
    assert!(error.contains("version mismatch"), "{error}");
    assert_eq!(index, std::fs::read(&config.index).unwrap());
    std::fs::write(source.join("plugin.toml"), &manifest).unwrap();
    std::fs::write(source.join("runtime.lock"), "different frozen lock\n").unwrap();
    let error = registry::provision_bundled_with_pixi(&config, &id, &pixi)
        .unwrap_err()
        .to_string();
    assert!(error.contains("frozen runtime lock changed"), "{error}");
    assert_eq!(index, std::fs::read(&config.index).unwrap());
    std::fs::write(source.join("runtime.lock"), "controlled lock\n").unwrap();
    std::fs::write(
        source.join("plugin.toml"),
        format!("{manifest}\n# new package revision\n"),
    )
    .unwrap();
    registry::provision_bundled_with_pixi(&config, &id, &pixi).unwrap();
    let updated = Registry::load(&config).unwrap();
    let next = updated.packages[&id]
        .manifest
        .managed_runtime
        .as_ref()
        .unwrap();
    assert_ne!(selected.root, next.root);
    selected.verify().unwrap();
    executable(
        &selected.root.join(".pixi/envs/default/bin/nativecheck"),
        "#!/bin/sh\necho changed\n",
    );
    assert!(
        selected
            .verify()
            .unwrap_err()
            .to_string()
            .contains("executable changed")
    );
}

#[test]
fn bundled_runtime_requires_frozen_lock_and_rejects_traversal() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let mut config = config(temp.path());
    let source = source(temp.path(), DECLARED);
    config.bundled = temp.path().to_owned();
    let id = Manifest::load(&source).unwrap().id;
    let error = registry::provision_bundled_with_pixi(&config, &id, Path::new("/missing-pixi"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("frozen runtime lock"), "{error}");
    assert!(!config.index.exists());
    let path = source.join("plugin.toml");
    let manifest = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        manifest.replace("[runtime]\n", "[runtime]\nlock = \"../outside.lock\"\n"),
    )
    .unwrap();
    assert!(
        Manifest::load(&source)
            .unwrap_err()
            .to_string()
            .contains("package-relative")
    );
}

#[test]
fn source_table2asn_requires_qualified_deployment_without_running_pixi() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let mut config = config(temp.path());
    config.bundled = temp.path().join("bundled");
    let plugin = config.bundled.join("table2asn");
    std::fs::create_dir_all(&plugin).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/table2asn");
    for file in ["plugin.toml", "operations.json", "table2asn_plugin.py"] {
        std::fs::copy(source.join(file), plugin.join(file)).unwrap();
    }
    let manifest = Manifest::load(&plugin).unwrap();
    let Some(runtime) = manifest.runtime else {
        return; // The pinned runtime is currently Linux x86_64 only.
    };
    assert!(runtime.lock.is_none());
    let error = registry::provision_bundled_with_pixi(
        &config,
        "basecraft.table2asn",
        Path::new("/must-not-run-pixi"),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("stage a qualified deployment package"),
        "{error}"
    );
    assert!(!config.index.exists());
    assert!(!config.packages.exists());
}

#[test]
fn local_channels_are_data_only_and_target_gating_is_explicit() {
    use basecraft_plugin_host::runtime::RuntimeSpec;
    crate::isolated_profile::isolate!();
    for channel in [
        "conda-forge",
        "/media/runtime/channel",
        "file:///media/runtime/channel",
    ] {
        let spec: RuntimeSpec = toml::from_str(&format!(
            "channels = [{channel:?}]\nplatforms = [\"linux-64\"]\n"
        ))
        .unwrap();
        spec.validate().unwrap();
        assert!(spec.supports("linux-64"));
        assert!(!spec.supports("win-64"));
    }
    for channel in [
        "https://example.org/channel",
        "file://server/channel",
        "relative/channel",
        "file:///media/../private",
        "/media/./channel",
        "file:///media/%2e%2e",
        "/media/channel?query",
        "/media/channel#fragment",
        "/media/$(command)",
    ] {
        let spec: RuntimeSpec = toml::from_str(&format!("channels = [{channel:?}]\n")).unwrap();
        assert!(spec.validate().is_err(), "{channel}");
    }
    let temp = tempfile::tempdir().unwrap();
    let source = source(temp.path(), "[runtime]\nplatforms = [\"osx-arm64\"]\n");
    if basecraft_plugin_host::runtime::current_platform() != "osx-arm64" {
        assert!(Manifest::load(&source).unwrap().runtime.is_none());
    }
    std::fs::write(source.join("plugin.toml"), "invalid").unwrap();
    assert!(Manifest::load(&source).is_err());
}
