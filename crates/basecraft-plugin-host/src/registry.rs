//! Read-only discovery and atomic managed-package selection. No install hooks.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use basecraft_platform::paths::AppPaths;
use basecraft_protocol::operation::{MAX_DESCRIPTOR_BYTES, OperationDescriptor, OperationPins};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Manifest, Result, contract};

pub const INSTALL_WARNING: &str = "Third-party plugins run code with your user account's permissions. Install only code you trust. You are responsible for checking that the plugin's code is safe.";
const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PACKAGE_FILES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Bundled,
    Managed,
    Editable,
}

#[derive(Clone, Debug)]
pub struct RegistryConfig {
    pub bundled: PathBuf,
    pub packages: PathBuf,
    pub index: PathBuf,
}

impl RegistryConfig {
    pub fn discover() -> Result<Self> {
        let paths = AppPaths::discover().map_err(|e| Error::Protocol(e.to_string()))?;
        let bundled = crate::installation::plugins();
        Ok(Self {
            bundled,
            packages: paths.data.join("plugins"),
            index: paths.config.join("plugins-v1.json"),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Package {
    pub manifest: Manifest,
    pub origin: Origin,
    pub digest: String,
    pub enabled: bool,
    pub descriptors: Vec<OperationDescriptor>,
    pub interpreter: Option<PathBuf>,
}

impl Package {
    pub fn verify(&self) -> Result<()> {
        if package_digest(&self.manifest.dir)? != self.digest {
            return Err(Error::Protocol(
                "plugin_changed: package contents changed; refresh registry".into(),
            ));
        }
        if let Some(runtime) = &self.manifest.managed_runtime {
            runtime.verify()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct RegisteredOperation {
    pub package: Arc<Package>,
    pub descriptor: OperationDescriptor,
    pub id: String,
    pub descriptor_digest: String,
}

impl RegisteredOperation {
    pub fn pins(&self, generation: &str) -> OperationPins {
        OperationPins {
            generation: generation.into(),
            package_digest: self.package.digest.clone(),
            descriptor_digest: self.descriptor_digest.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Registry {
    pub generation: String,
    pub packages: BTreeMap<String, Arc<Package>>,
    pub operations: BTreeMap<String, RegisteredOperation>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct InstallationIndex {
    #[serde(default = "index_version")]
    schema: u32,
    #[serde(default)]
    active: BTreeMap<String, String>,
    #[serde(default)]
    runtimes: BTreeMap<String, crate::runtime::Runtime>,
    #[serde(default)]
    disabled: BTreeSet<String>,
    #[serde(default)]
    editable: BTreeSet<PathBuf>,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}
fn index_version() -> u32 {
    1
}

fn load_index(config: &RegistryConfig) -> Result<InstallationIndex> {
    let file = match std::fs::File::open(&config.index) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InstallationIndex {
                schema: 1,
                ..InstallationIndex::default()
            });
        }
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take((MAX_DESCRIPTOR_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DESCRIPTOR_BYTES {
        return Err(Error::Protocol("plugin index exceeds size limit".into()));
    }
    let index: InstallationIndex = serde_json::from_slice(&bytes)?;
    if index.schema != 1 {
        return Err(Error::Protocol("unsupported plugin index version".into()));
    }
    Ok(index)
}

impl Registry {
    /// Build a new immutable generation; listing creates no directories or records.
    pub fn load(config: &RegistryConfig) -> Result<Self> {
        let index = load_index(config)?;
        let mut candidates = Vec::new();
        let mut diagnostics = Vec::new();
        match std::fs::read_dir(&config.bundled) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    if entry.file_type()?.is_dir() {
                        candidates.push((entry.path(), Origin::Bundled, None));
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        for (id, relative) in &index.active {
            let path = Path::new(relative);
            if path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                diagnostics.push(Diagnostic {
                    path: config.index.clone(),
                    message: format!("invalid installed path for {id}"),
                });
                continue;
            }
            candidates.push((
                config.packages.join(path),
                Origin::Managed,
                Some(id.clone()),
            ));
        }
        candidates.extend(
            index
                .editable
                .iter()
                .cloned()
                .map(|p| (p, Origin::Editable, None)),
        );
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        let mut by_id: BTreeMap<String, Vec<Arc<Package>>> = BTreeMap::new();
        for (path, origin, expected_id) in candidates {
            let package = (|| -> Result<Package> {
                let mut manifest = Manifest::load(&path)?;
                contract::validate_plugin_id(&manifest.id)?;
                if origin != Origin::Bundled && manifest.id.starts_with("basecraft.") {
                    return Err(Error::Protocol(
                        "basecraft.* identities are reserved for bundled packages".into(),
                    ));
                }
                if expected_id.as_ref().is_some_and(|id| id != &manifest.id) {
                    return Err(Error::Protocol(
                        "installed package identity mismatch".into(),
                    ));
                }
                let descriptors = manifest
                    .operation_contract()?
                    .map_or_else(Vec::new, |c| c.operations);
                let digest = package_digest(&path)?;
                let enabled = !index.disabled.contains(&manifest.id);
                let interpreter = if manifest.runtime.is_some() {
                    let key = runtime_key(&manifest.id, &digest);
                    let directory = if origin == Origin::Bundled {
                        Some(&key)
                    } else {
                        index.active.get(&manifest.id)
                    };
                    let runtime = directory.and_then(|d| index.runtimes.get(d)).filter(|r| {
                        matches!(origin, Origin::Managed | Origin::Bundled) && r.package_digest == digest
                    }).ok_or_else(|| Error::Protocol("managed plugin runtime unavailable; install or reinstall this package to provision dependencies".into()))?;
                    runtime.verify()?;
                    manifest.managed_runtime = Some(runtime.clone());
                    Some(runtime.interpreter.clone())
                } else {
                    configured_python(origin)
                };
                Ok(Package {
                    manifest,
                    origin,
                    digest,
                    enabled,
                    descriptors,
                    interpreter,
                })
            })();
            match package {
                Ok(package) => by_id
                    .entry(package.manifest.id.clone())
                    .or_default()
                    .push(Arc::new(package)),
                Err(e) => diagnostics.push(Diagnostic {
                    path,
                    message: e.to_string(),
                }),
            }
        }
        let mut packages = BTreeMap::new();
        let mut operations = BTreeMap::new();
        for (id, entries) in by_id {
            if entries.len() != 1 {
                for package in entries {
                    diagnostics.push(Diagnostic {
                        path: package.manifest.dir.clone(),
                        message: format!(
                            "duplicate active package identity {id}; no version selected"
                        ),
                    });
                }
                continue;
            }
            let package = entries[0].clone();
            for descriptor in &package.descriptors {
                let id = contract::operation_id(&package.manifest.id, descriptor)?;
                let descriptor_digest = basecraft_io::hash_bytes(&serde_json::to_vec(descriptor)?);
                operations.insert(
                    id.clone(),
                    RegisteredOperation {
                        package: package.clone(),
                        descriptor: descriptor.clone(),
                        id,
                        descriptor_digest,
                    },
                );
            }
            packages.insert(id, package);
        }
        diagnostics.sort_by(|a, b| (&a.path, &a.message).cmp(&(&b.path, &b.message)));
        let identity: Vec<_> = packages
            .iter()
            .map(|(id, p)| {
                (
                    id,
                    &p.digest,
                    p.enabled,
                    &p.interpreter,
                    &p.manifest.managed_runtime,
                )
            })
            .collect();
        let generation = basecraft_io::hash_bytes(&serde_json::to_vec(&(&identity, &diagnostics))?);
        Ok(Self {
            generation,
            packages,
            operations,
            diagnostics,
        })
    }

    pub fn resolve(&self, id: &str, pins: Option<&OperationPins>) -> Result<&RegisteredOperation> {
        let operation = self.operations.get(id).ok_or_else(|| {
            Error::Protocol(format!("plugin_unavailable: unknown operation {id}"))
        })?;
        if !operation.package.enabled {
            return Err(Error::Protocol("plugin_disabled".into()));
        }
        if pins.is_some_and(|p| p != &operation.pins(&self.generation)) {
            return Err(Error::Protocol(
                "plugin_changed: stale registry pins; discover again".into(),
            ));
        }
        if operation.package.interpreter.is_none() {
            return Err(Error::Protocol(
                "plugin_unavailable: configure BASECRAFT_PYTHON or install python3".into(),
            ));
        }
        operation.package.verify()?;
        Ok(operation)
    }
}

/// Complete-package fingerprint; never imports a script. Symlinks/special files
/// are rejected rather than following unrelated local data into an installation.
pub fn package_digest(root: &Path) -> Result<String> {
    let files = package_files(root)?;
    let mut hashes = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let name = relative
            .to_str()
            .ok_or_else(|| Error::Protocol("package paths must be UTF-8".into()))?;
        hashes.push((
            name.to_owned(),
            basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)?,
        ));
    }
    Ok(basecraft_io::hash_bytes(&serde_json::to_vec(&hashes)?))
}

fn package_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(dir: &Path, files: &mut Vec<PathBuf>, bytes: &mut u64, depth: usize) -> Result<()> {
        if depth > 32 {
            return Err(Error::Protocol(
                "package directory depth limit exceeded".into(),
            ));
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let metadata = entry.path().symlink_metadata()?;
            if metadata.is_dir() {
                visit(&entry.path(), files, bytes, depth + 1)?;
            } else if metadata.is_file() {
                *bytes = bytes.saturating_add(metadata.len());
                files.push(entry.path());
                if files.len() > MAX_PACKAGE_FILES || *bytes > MAX_PACKAGE_BYTES {
                    return Err(Error::Protocol("package file/byte limit exceeded".into()));
                }
            } else {
                return Err(Error::Protocol(
                    "plugin package contains a symbolic link or special file".into(),
                ));
            }
        }
        Ok(())
    }
    if root.symlink_metadata()?.file_type().is_symlink() {
        return Err(Error::Protocol(
            "plugin package root cannot be a symbolic link".into(),
        ));
    }
    let mut files = Vec::new();
    visit(root, &mut files, &mut 0, 0)?;
    files.sort();
    Ok(files)
}

fn write_index(
    config: &RegistryConfig,
    change: impl FnOnce(&mut InstallationIndex) -> Result<()>,
) -> Result<()> {
    let parent = config
        .index
        .parent()
        .ok_or_else(|| Error::Protocol("index needs parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    let lock = config.index.with_extension("lock");
    let index_lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
        .map_err(|e| {
            Error::Protocol(format!(
                "cannot lock plugin index (another install may be active): {e}"
            ))
        })?;
    if !index_lock.try_lock_exclusive()? {
        return Err(Error::Protocol(
            "plugin index is locked by another installation".into(),
        ));
    }
    let mut index = load_index(config)?;
    change(&mut index)?;
    let bytes = serde_json::to_vec_pretty(&index)?;
    if bytes.len() > MAX_DESCRIPTOR_BYTES {
        return Err(Error::Protocol("plugin index exceeds size limit".into()));
    }
    basecraft_io::write_atomic(&config.index, |out| std::io::Write::write_all(out, &bytes))
        .map_err(basecraft_ops::Error::from)?;
    Ok(())
}

fn runtime_key(id: &str, digest: &str) -> String {
    format!("{id}-{}", digest.replace(':', "-"))
}

/// Explicit deployment step for bundled dependencies, shared by GUI/CLI/MCP
/// embedders. Registry discovery remains read-only and never accesses a network.
/// The digest key retains side-by-side selections for older bundles/jobs.
pub fn provision_bundled(config: &RegistryConfig, id: &str) -> Result<()> {
    provision_bundled_with_pixi(config, id, &crate::runtime::pixi())
}

pub fn provision_bundled_with_pixi(config: &RegistryConfig, id: &str, pixi: &Path) -> Result<()> {
    contract::validate_plugin_id(id)?;
    let source = std::fs::read_dir(&config.bundled)?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| Manifest::load(&entry.path()).ok())
        .filter(|manifest| manifest.id == id)
        .collect::<Vec<_>>();
    if source.len() != 1 {
        return Err(Error::Protocol(
            "expected one bundled package with this identity".into(),
        ));
    }
    let manifest = &source[0];
    let spec = manifest
        .runtime
        .as_ref()
        .ok_or_else(|| Error::Protocol("bundled package has no declared runtime".into()))?;
    if spec.lock.is_none() {
        return Err(Error::Protocol(
            "bundled dependencies require a frozen runtime lock; no latest fallback".into(),
        ));
    }
    let digest = package_digest(&manifest.dir)?;
    let key = runtime_key(id, &digest);
    if let Some(runtime) = load_index(config)?.runtimes.get(&key) {
        if runtime.package_digest != digest {
            return Err(Error::Protocol(
                "bundled runtime package digest mismatch".into(),
            ));
        }
        runtime.verify()?;
        return Ok(());
    }
    let root = config
        .packages
        .parent()
        .ok_or_else(|| Error::Protocol("packages need parent directory".into()))?
        .join("plugin-runtimes");
    let runtime = crate::runtime::provision_with(manifest, &digest, &root, pixi)?
        .ok_or_else(|| Error::Protocol("bundled runtime missing".into()))?;
    if package_digest(&manifest.dir)? != digest {
        return Err(Error::Protocol(
            "plugin_changed: bundled package changed during provisioning".into(),
        ));
    }
    write_index(config, |index| {
        index.runtimes.insert(key, runtime);
        Ok(())
    })
}

/// The caller displays `INSTALL_WARNING` during the explicit install action.
/// Copies are immutable by convention and never removed while jobs might use them.
pub fn install(config: &RegistryConfig, source: &Path) -> Result<String> {
    install_with_pixi(config, source, &crate::runtime::pixi())
}

/// Explicit tool selection for embedding and controlled installer tests.
pub fn install_with_pixi(config: &RegistryConfig, source: &Path, pixi: &Path) -> Result<String> {
    let manifest = Manifest::load(source)?;
    contract::validate_plugin_id(&manifest.id)?;
    if manifest.id.starts_with("basecraft.") {
        return Err(Error::Protocol(
            "basecraft.* is reserved for bundled packages".into(),
        ));
    }
    let digest = package_digest(source)?;
    let directory = format!("{}-{}", manifest.id, digest.replace(':', "-"));
    std::fs::create_dir_all(&config.packages)?;
    let destination = config.packages.join(&directory);
    let pending = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(&config.packages)?;
    for file in package_files(source)? {
        let relative = file
            .strip_prefix(source)
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let target = pending.path().join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file, target)?;
    }
    if package_digest(pending.path())? != digest {
        return Err(Error::Protocol(
            "plugin_changed: package changed during installation".into(),
        ));
    }
    let snapshot = Manifest::load(pending.path())?;
    let runtime_root = config
        .packages
        .parent()
        .ok_or_else(|| Error::Protocol("packages need parent directory".into()))?
        .join("plugin-runtimes");
    let runtime = crate::runtime::provision_with(&snapshot, &digest, &runtime_root, pixi)?;
    // Serialize package publication and runtime selection together. Any failure
    // leaves the old index/package/runtime available to both new and owned jobs.
    write_index(config, |index| {
        if destination.exists() {
            if package_digest(&destination)? != digest {
                return Err(Error::Protocol("installed package digest mismatch".into()));
            }
        } else {
            std::fs::rename(pending.path(), &destination)?;
        }
        if let Some(runtime) = &runtime {
            index.runtimes.insert(directory.clone(), runtime.clone());
        }
        index.active.insert(manifest.id.clone(), directory.clone());
        Ok(())
    })?;
    Ok(manifest.id)
}

pub fn set_enabled(config: &RegistryConfig, id: &str, enabled: bool) -> Result<()> {
    if !Registry::load(config)?.packages.contains_key(id) {
        return Err(Error::Protocol("unknown plugin".into()));
    }
    write_index(config, |index| {
        if enabled {
            index.disabled.remove(id);
        } else {
            index.disabled.insert(id.into());
        }
        Ok(())
    })
}

pub fn remove(config: &RegistryConfig, id: &str) -> Result<()> {
    write_index(config, |index| {
        if index.active.remove(id).is_none() {
            return Err(Error::Protocol("unknown managed plugin".into()));
        }
        Ok(())
    })
}

pub fn register_editable(config: &RegistryConfig, path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    let manifest = Manifest::load(&path)?;
    contract::validate_plugin_id(&manifest.id)?;
    if manifest.id.starts_with("basecraft.") {
        return Err(Error::Protocol("basecraft.* is reserved".into()));
    }
    package_digest(&path)?;
    write_index(config, |index| {
        index.editable.insert(path);
        Ok(())
    })
}

/// Freeze the local runtime selection in this registry generation. Third-party
/// installs do not scan arbitrary pixi environments; `BASECRAFT_PYTHON` is explicit.
fn configured_python(origin: Origin) -> Option<PathBuf> {
    let selected = if origin == Origin::Bundled {
        crate::process::python_interpreter()
    } else {
        std::env::var_os("BASECRAFT_PYTHON").map_or_else(|| PathBuf::from("python3"), PathBuf::from)
    };
    if selected.is_absolute() {
        return selected.canonicalize().ok().filter(|p| p.is_file());
    }
    if selected.components().count() != 1 {
        return None;
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(&selected))
            .find(|path| path.is_file())
            .and_then(|path| path.canonicalize().ok())
    })
}

/// Every invocation route, including explicit legacy facades, honors the same
/// local disabled choice. This reads configuration without creating it.
pub fn require_enabled(id: &str) -> Result<()> {
    let config = RegistryConfig::discover()?;
    if load_index(&config)?.disabled.contains(id) {
        return Err(Error::Protocol("plugin_disabled".into()));
    }
    Ok(())
}
