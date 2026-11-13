//! Install-time Pixi provisioning. Discovery only reads retained receipts/locks.
use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use basecraft_platform::process::OwnedProcess;
use serde::{Deserialize, Serialize};

use crate::{Error, Manifest, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RuntimeSpec {
    /// Empty preserves existing all-platform declarations. Other targets keep
    /// the plugin's unmanaged behavior; no substitute package is provisioned.
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default = "channels")]
    pub channels: Vec<String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub pypi_dependencies: BTreeMap<String, String>,
    /// Commands checked within the selected environment, never on ambient PATH.
    #[serde(default)]
    pub executables: Vec<String>,
    /// Package-relative frozen Pixi lock. Install never updates this selection.
    #[serde(default)]
    pub lock: Option<String>,
    /// Exact trimmed stdout+stderr, captured on the selected native build.
    #[serde(default)]
    pub version_checks: BTreeMap<String, VersionCheck>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct VersionCheck {
    pub args: Vec<String>,
    pub expected: String,
}

fn channels() -> Vec<String> {
    vec!["conda-forge".into()]
}
fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        && value != "."
        && value != ".."
}
impl RuntimeSpec {
    pub fn supports(&self, platform: &str) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|p| p == platform)
    }

    pub fn validate(&self) -> Result<()> {
        if self.platforms.len() > 5
            || self.platforms.iter().any(|p| {
                !matches!(
                    p.as_str(),
                    "linux-64" | "linux-aarch64" | "osx-64" | "osx-arm64" | "win-64"
                )
            })
        {
            return Err(Error::Protocol("invalid runtime target platforms".into()));
        }
        if self.channels.is_empty()
            || self.channels.len() > 16
            || self.channels.iter().any(|c| !channel(c))
            || self.dependencies.len() + self.pypi_dependencies.len() > 256
            || self.executables.len() > 128
            || self.executables.iter().any(|c| !name(c))
        {
            return Err(Error::Protocol("invalid runtime channels/dependency/command limits (channels must be named or absolute local file channels)".into()));
        }
        for (package, version) in self.dependencies.iter().chain(&self.pypi_dependencies) {
            if !name(package)
                || version.is_empty()
                || version.len() > 256
                || version.chars().any(char::is_control)
            {
                return Err(Error::Protocol(
                    "runtime dependencies require package names and bounded version strings".into(),
                ));
            }
        }
        if self.version_checks.len() > 128
            || self.version_checks.iter().any(|(command, check)| {
                !self.executables.contains(command)
                    || check.args.len() > 16
                    || check
                        .args
                        .iter()
                        .any(|a| a.len() > 256 || a.chars().any(char::is_control))
                    || check.expected.is_empty()
                    || check.expected.len() > 8192
            })
        {
            return Err(Error::Protocol("invalid runtime version checks".into()));
        }
        if let Some(lock) = &self.lock
            && (lock.is_empty()
                || lock.len() > 256
                || Path::new(lock)
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_))))
        {
            return Err(Error::Protocol(
                "runtime lock must be a package-relative path".into(),
            ));
        }
        // These are shared SDK dependencies. Mixing conda and PyPI ownership of
        // them can overwrite the solver's Python/Arrow ABI choices.
        if self.pypi_dependencies.keys().any(|p| {
            matches!(
                p.to_ascii_lowercase().as_str(),
                "python" | "pyarrow" | "numpy"
            )
        }) {
            return Err(Error::Protocol(
                "declare Python, pyarrow and numpy constraints in runtime.dependencies".into(),
            ));
        }
        Ok(())
    }

    fn manifest(&self) -> Result<String> {
        self.validate()?;
        let mut dependencies = self.dependencies.clone();
        dependencies
            .entry("python".into())
            .or_insert_with(|| ">=3.11,<3.14".into());
        dependencies
            .entry("pyarrow".into())
            .or_insert_with(|| "*".into());
        dependencies
            .entry("numpy".into())
            .or_insert_with(|| "*".into());
        let platform = current_platform();
        if platform == "unsupported" || !self.supports(platform) {
            return Err(Error::Protocol(
                "managed plugin runtime unsupported on this platform".into(),
            ));
        }
        let value = serde_json::json!({
            "workspace":{"name":"basecraft-plugin", "channels":self.channels,"platforms":[platform]},
            "dependencies": dependencies,
            "pypi-dependencies": self.pypi_dependencies,
        });
        let value = toml::Value::try_from(&value).map_err(|e| Error::Protocol(e.to_string()))?;
        toml::to_string(&value).map_err(|e| Error::Protocol(e.to_string()))
    }
}

// A local feedback channel is data for Pixi's frozen installer, never a shell
// command. Remote URLs, URI authorities, query/fragment/escaped paths and path
// traversal are deliberately outside this contract.
fn channel(value: &str) -> bool {
    if name(value) {
        return true;
    }
    let path = value.strip_prefix("file://").unwrap_or(value);
    path.starts_with('/')
        && !path.starts_with("//")
        && path.len() <= 4096
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
        && Path::new(path).components().all(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
        && path.split('/').all(|c| c != "." && c != "..")
}

pub fn current_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "aarch64") => "osx-arm64",
        ("macos", "x86_64") => "osx-64",
        ("windows", "x86_64") => "win-64",
        _ => "unsupported",
    }
}

/// Immutable selection retained by a registry generation and accepted jobs.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Runtime(pub basecraft_protocol::installation_records::Runtime);

impl std::ops::Deref for Runtime {
    type Target = basecraft_protocol::installation_records::Runtime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Runtime {
    pub fn verify(&self) -> Result<()> {
        let hash = |path: PathBuf| -> Result<String> {
            basecraft_io::hash_file(&path)
                .map_err(basecraft_ops::Error::from)
                .map_err(Into::into)
        };
        if self.schema != 1
            || self.executables.iter().any(|n| !name(n))
            || !self.root.is_absolute()
            || self.interpreter != interpreter_path(&self.root)
        {
            return Err(Error::Protocol(
                "invalid managed runtime prefix; reinstall the plugin".into(),
            ));
        }
        for executable in std::iter::once(self.interpreter.clone()).chain(
            self.executables
                .iter()
                .map(|n| executable_path(&self.root, n)),
        ) {
            if !executable.metadata().is_ok_and(|m| is_executable(&m)) {
                return Err(Error::Protocol(format!(
                    "managed plugin runtime executable missing or not executable: {}; reinstall the plugin",
                    executable.display()
                )));
            }
        }
        for (command, digest) in &self.executable_digests {
            if !self.executables.contains(command)
                || hash(executable_path(&self.root, command))? != *digest
            {
                return Err(Error::Protocol(
                    "managed runtime executable changed; reinstall the plugin".into(),
                ));
            }
        }
        if !self.interpreter.is_file()
            || hash(self.root.join("pixi.lock")).map_err(|e| {
                Error::Protocol(format!(
                    "managed runtime lock unavailable: {e}; reinstall the plugin"
                ))
            })? != self.lock_digest
            || hash(self.root.join("pixi.toml")).map_err(|e| {
                Error::Protocol(format!(
                    "managed runtime manifest unavailable: {e}; reinstall the plugin"
                ))
            })? != self.manifest_digest
        {
            return Err(Error::Protocol(
                "managed plugin runtime missing or changed; reinstall the plugin".into(),
            ));
        }
        Ok(())
    }
}

fn environment_paths(root: &Path) -> Vec<PathBuf> {
    let prefix = root.join(".pixi/envs/default");
    if cfg!(windows) {
        vec![
            prefix.clone(),
            prefix.join("Scripts"),
            prefix.join("Library/bin"),
        ]
    } else {
        vec![prefix.join("bin")]
    }
}
fn interpreter_path(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join(".pixi/envs/default/python.exe")
    } else {
        root.join(".pixi/envs/default/bin/python")
    }
}
fn executable_path(root: &Path, name: &str) -> PathBuf {
    let file = if cfg!(windows)
        && !Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let paths = environment_paths(root);
    paths
        .iter()
        .map(|p| p.join(&file))
        .find(|p| p.is_file())
        .unwrap_or_else(|| {
            let prefix = root.join(".pixi/envs/default");
            if cfg!(windows) {
                prefix.join(file)
            } else {
                prefix.join("bin").join(file)
            }
        })
}
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// `BASECRAFT_PIXI` is an explicit developer override; deployed tool wins over PATH.
pub fn pixi() -> PathBuf {
    if let Some(path) = std::env::var_os("BASECRAFT_PIXI") {
        return path.into();
    }
    let name = if cfg!(windows) { "pixi.exe" } else { "pixi" };
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin) = exe.parent()
        && bin.join(name).is_file()
    {
        return bin.join(name);
    }
    PathBuf::from(name)
}

/// Every attempt has its final prefix from the start: conda prefixes cannot be
/// renamed. Failed attempts are disposable; successful prefixes are retained.
pub fn provision(manifest: &Manifest, digest: &str, root: &Path) -> Result<Option<Runtime>> {
    provision_with(manifest, digest, root, &pixi())
}

/// Explicit tool injection supports controlled tests without process-global overrides.
pub fn provision_with(
    manifest: &Manifest,
    digest: &str,
    root: &Path,
    pixi: &Path,
) -> Result<Option<Runtime>> {
    let Some(spec) = &manifest.runtime else {
        return Ok(None);
    };
    checked(
        Command::new(pixi).arg("--version"),
        Duration::from_secs(15),
        &format!(
            "plugin provisioner unavailable or broken ({}); repair the Basecraft installation",
            pixi.display()
        ),
    )?;
    std::fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let pending = tempfile::Builder::new()
        .prefix("runtime-")
        .tempdir_in(root)?;
    let project = pending.path().join("pixi.toml");
    std::fs::write(&project, spec.manifest()?)?;
    let frozen_lock_digest = if let Some(lock) = &spec.lock {
        let source = manifest.dir.join(lock).canonicalize()?;
        if !source.starts_with(manifest.dir.canonicalize()?) {
            return Err(Error::Protocol(
                "runtime lock escapes plugin package".into(),
            ));
        }
        std::fs::copy(source, pending.path().join("pixi.lock"))?;
        Some(basecraft_io::hash_file(&pending.path().join("pixi.lock"))?)
    } else {
        None
    };
    // Override detached user environments. No source-provided Pixi config,
    // tasks, activation hooks, global pip or shell commands are accepted.
    std::fs::create_dir(pending.path().join(".pixi"))?;
    std::fs::write(
        pending.path().join(".pixi/config.toml"),
        "detached-environments = false\n",
    )?;
    let mut install = Command::new(pixi);
    install
        .args(["install", "--no-config", "--manifest-path"])
        .arg(&project)
        .args([
            "--color",
            "never",
            "--concurrent-solves",
            "2",
            "--pypi-keyring-provider",
            "disabled",
        ])
        .current_dir(pending.path());
    if spec.lock.is_some() {
        install.arg("--locked");
    }
    checked(
        &mut install,
        Duration::from_secs(1800),
        "plugin dependency installation (Pixi must be included in the Basecraft installation)",
    )?;
    if let Some(digest) = frozen_lock_digest
        && digest != basecraft_io::hash_file(&pending.path().join("pixi.lock"))?
    {
        return Err(Error::Protocol(
            "frozen runtime lock changed during installation".into(),
        ));
    }
    let interpreter = interpreter_path(pending.path());
    if !interpreter.is_file() {
        return Err(Error::Protocol(
            "Pixi did not install the declared plugin interpreter".into(),
        ));
    }
    let mut verify = Command::new(&interpreter);
    verify.args(["-I", "-c", "import sys, pyarrow, numpy, shutil, importlib.metadata as m, json; assert (3,11) <= sys.version_info[:2] < (3,14), 'SDK requires Python 3.11–3.13'; [m.version(p) for p in json.loads(sys.argv[1])]; missing=[p for p in json.loads(sys.argv[2]) if shutil.which(p) is None]; assert not missing, 'missing runtime executables: '+str(missing)"])
        .arg(serde_json::to_string(&spec.pypi_dependencies.keys().collect::<Vec<_>>())?)
        .arg(serde_json::to_string(&spec.executables)?);
    // Verification cannot satisfy commands from ambient PATH.
    verify.env(
        "PATH",
        std::env::join_paths(environment_paths(pending.path()))
            .map_err(|e| Error::Protocol(e.to_string()))?,
    );
    checked(
        &mut verify,
        Duration::from_secs(60),
        "plugin dependency verification",
    )?;
    let mut reported_versions = BTreeMap::new();
    let mut executable_digests = BTreeMap::new();
    for command in &spec.executables {
        let path = executable_path(pending.path(), command);
        executable_digests.insert(command.clone(), basecraft_io::hash_file(&path)?);
        if let Some(check) = spec.version_checks.get(command) {
            // No shell or ambient executable resolution. The Python subprocess
            // enforces a deadline; the shared owner also bounds the outer job.
            let mut verify = Command::new(&interpreter);
            verify.args(["-I", "-c", "import subprocess,sys,json; p=subprocess.run([sys.argv[1]]+json.loads(sys.argv[2]),capture_output=True,text=True,timeout=30); actual=(p.stdout+p.stderr).strip(); assert p.returncode==0 and actual==sys.argv[3], 'runtime version mismatch: '+actual[:8192]"])
                .arg(path).arg(serde_json::to_string(&check.args)?).arg(&check.expected)
                .env("PATH", std::env::join_paths(environment_paths(pending.path()))
                    .map_err(|e| Error::Protocol(e.to_string()))?);
            checked(
                &mut verify,
                Duration::from_secs(40),
                "plugin exact version verification",
            )?;
            reported_versions.insert(command.clone(), check.expected.clone());
        }
    }
    let runtime = Runtime(basecraft_protocol::installation_records::Runtime {
        schema: 1,
        executables: spec.executables.clone(),
        root: pending.path().to_owned(),
        interpreter,
        lock_digest: basecraft_io::hash_file(&pending.path().join("pixi.lock"))?,
        manifest_digest: basecraft_io::hash_file(&project)?,
        package_digest: digest.into(),
        reported_versions,
        executable_digests,
    });
    runtime.verify()?;
    basecraft_io::write_atomic(&pending.path().join("runtime.json"), |out| {
        out.write_all(&basecraft_protocol::records::encode(&runtime)?)
    })?;
    let _retained = pending.keep();
    Ok(Some(runtime))
}

/// Bounded diagnostics and process ownership also apply to package tooling.
fn checked(command: &mut Command, timeout: Duration, label: &str) -> Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child =
        OwnedProcess::spawn(command).map_err(|e| Error::Protocol(format!("{label}: {e}")))?;
    let output = std::sync::Arc::new(std::sync::Mutex::new(
        crate::process::CapturedOutput::default(),
    ));
    let mut workers = Vec::new();
    if let Some(stdout) = child.take_stdout() {
        workers.push(crate::process::capture_output(
            stdout,
            output.clone(),
            false,
        )?);
    }
    if let Some(stderr) = child.take_stderr() {
        workers.push(crate::process::capture_output(
            stderr,
            output.clone(),
            true,
        )?);
    }
    let start = Instant::now();
    while !child.exited()? {
        if start.elapsed() >= timeout {
            child.terminate();
            return Err(Error::Protocol(format!(
                "{label} timed out; prior installation unchanged"
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = child.finish()?;
    // A descendant retaining pipes must not block install indefinitely.
    let drain = Instant::now();
    while workers.iter().any(|w| !w.is_finished()) && drain.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(5));
    }
    for worker in workers {
        if worker.is_finished() {
            let _ = worker.join();
        }
    }
    if !status.success() {
        let output = output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        return Err(Error::Protocol(format!(
            "{label} failed ({status}): {}",
            String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(8192)]).to_string()
                + &String::from_utf8_lossy(&output.stdout[..output.stdout.len().min(8192)])
        )));
    }
    Ok(())
}

/// Selected environment commands win over package artifacts and ambient tools.
/// The bundled interpreter follows the same rule as a managed interpreter.
pub fn configure_command(command: &mut Command, manifest: &Manifest, python: &Path) -> Result<()> {
    let mut paths = manifest
        .managed_runtime
        .as_ref()
        .map_or_else(Vec::new, |runtime| environment_paths(&runtime.root));
    if python.is_absolute()
        && let Some(bin) = python.parent()
    {
        paths.push(bin.to_owned());
        #[cfg(windows)]
        {
            paths.push(bin.join("Scripts"));
            paths.push(bin.join("Library/bin"));
        }
    }
    #[cfg(windows)]
    if let Ok(executable) = std::env::current_exe()
        && let Some(bin) = executable.parent()
    {
        paths.push(bin.to_owned());
    }
    let package_bin = manifest.dir.join("bin");
    if package_bin.is_dir() {
        paths.push(package_bin);
    }
    if let Some(ambient) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&ambient));
    }
    // Only host-selected absolute executables may satisfy a pinned runtime.
    // Clear inherited declarations for plugins without a selected runtime.
    command.env_remove("BASECRAFT_RUNTIME_EXECUTABLES");
    if let Some(runtime) = &manifest.managed_runtime {
        let executables: BTreeMap<_, _> = runtime
            .executables
            .iter()
            .map(|name| (name, executable_path(&runtime.root, name)))
            .collect();
        command.env(
            "BASECRAFT_RUNTIME_EXECUTABLES",
            serde_json::to_string(&executables)?,
        );
    }
    let path = std::env::join_paths(paths).map_err(|e| Error::Protocol(e.to_string()))?;
    command
        .env("PATH", path)
        .env("PYTHONNOUSERSITE", "1")
        .env_remove("PYTHONHOME");
    Ok(())
}
