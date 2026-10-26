//! Network-free installer regression checks. Real Pixi invocation is a separate
//! opt-in test, using a disposable external profile and an actual channel solve.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use basecraft_plugin_host::{
    Manifest, PluginProcess,
    registry::{self, Registry, RegistryConfig},
};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/fixtures/registry-plugin")
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
    let mut context: basecraft_protocol::operation::InvocationContext = serde_json::from_value(serde_json::json!({
        "schema":1,"operation":"org.example.registry-fixture/count@1","run_id":"test-runtime",
        "inputs":{"sources":[temp.path().join("input")]},"outputs":{"report":temp.path().join("report")},
        "cpu_threads":1,"cancel_file":"","progress_file":""
    })).unwrap();
    context.cpu_threads = 1;
    let result = process
        .invoke_context(
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
}
