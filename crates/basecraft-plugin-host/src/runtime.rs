//! Install-time Pixi provisioning. Discovery only reads retained receipts/locks.
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use basecraft_platform::process::OwnedProcess;
use serde::{Deserialize, Serialize};

use crate::{Error, Manifest, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RuntimeSpec {
    #[serde(default = "channels")]
    pub channels: Vec<String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub pypi_dependencies: BTreeMap<String, String>,
    /// Commands checked within the selected environment, never on ambient PATH.
    #[serde(default)]
    pub executables: Vec<String>,
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
    pub fn validate(&self) -> Result<()> {
        if self.channels.is_empty()
            || self.channels.len() > 16
            || self.channels.iter().any(|c| !name(c))
            || self.dependencies.len() + self.pypi_dependencies.len() > 256
            || self.executables.len() > 128
            || self.executables.iter().any(|c| !name(c))
        {
            return Err(Error::Protocol("invalid runtime channels/dependency/command limits (channels must be named channels)".into()));
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
        let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => "linux-64",
            ("linux", "aarch64") => "linux-aarch64",
            ("macos", "aarch64") => "osx-arm64",
            ("macos", "x86_64") => "osx-64",
            _ => {
                return Err(Error::Protocol(
                    "managed plugin runtime unsupported on this platform".into(),
                ));
            }
        };
        let value = serde_json::json!({
            "workspace":{"name":"basecraft-plugin", "channels":self.channels,"platforms":[platform]},
            "dependencies": dependencies,
            "pypi-dependencies": self.pypi_dependencies,
        });
        let value = toml::Value::try_from(&value).map_err(|e| Error::Protocol(e.to_string()))?;
        toml::to_string(&value).map_err(|e| Error::Protocol(e.to_string()))
    }
}

/// Immutable selection retained by a registry generation and accepted jobs.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub schema: u32,
    pub executables: Vec<String>,
    pub root: PathBuf,
    pub interpreter: PathBuf,
    pub lock_digest: String,
    pub manifest_digest: String,
    pub package_digest: String,
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
            || self.interpreter != self.root.join(".pixi/envs/default/bin/python")
        {
            return Err(Error::Protocol(
                "invalid managed runtime prefix; reinstall the plugin".into(),
            ));
        }
        for executable in std::iter::once(self.interpreter.clone()).chain(
            self.executables
                .iter()
                .map(|n| self.root.join(".pixi/envs/default/bin").join(n)),
        ) {
            if !executable
                .metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            {
                return Err(Error::Protocol(format!(
                    "managed plugin runtime executable missing or not executable: {}; reinstall the plugin",
                    executable.display()
                )));
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

/// `BASECRAFT_PIXI` is an explicit developer override; deployed tool wins over PATH.
pub fn pixi() -> PathBuf {
    if let Some(path) = std::env::var_os("BASECRAFT_PIXI") {
        return path.into();
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin) = exe.parent()
        && bin.join("pixi").is_file()
    {
        return bin.join("pixi");
    }
    PathBuf::from("pixi")
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
    checked(
        &mut install,
        Duration::from_secs(1800),
        "plugin dependency installation (Pixi must be included in the Basecraft installation)",
    )?;
    let interpreter = pending.path().join(".pixi/envs/default/bin/python");
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
        interpreter
            .parent()
            .ok_or_else(|| Error::Protocol("runtime interpreter needs bin directory".into()))?,
    );
    checked(
        &mut verify,
        Duration::from_secs(60),
        "plugin dependency verification",
    )?;
    let runtime = Runtime {
        schema: 1,
        executables: spec.executables.clone(),
        root: pending.path().to_owned(),
        interpreter,
        lock_digest: basecraft_io::hash_file(&pending.path().join("pixi.lock"))
            .map_err(basecraft_ops::Error::from)?,
        manifest_digest: basecraft_io::hash_file(&project).map_err(basecraft_ops::Error::from)?,
        package_digest: digest.into(),
    };
    runtime.verify()?;
    basecraft_io::write_atomic(&pending.path().join("runtime.json"), |out| {
        serde_json::to_writer_pretty(out, &runtime).map_err(std::io::Error::other)
    })
    .map_err(basecraft_ops::Error::from)?;
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
    let mut paths = Vec::new();
    if python.is_absolute()
        && let Some(bin) = python.parent()
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
    let path = std::env::join_paths(paths).map_err(|e| Error::Protocol(e.to_string()))?;
    command
        .env("PATH", path)
        .env("PYTHONNOUSERSITE", "1")
        .env_remove("PYTHONHOME");
    Ok(())
}
