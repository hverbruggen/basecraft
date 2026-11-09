//! Matching deployed resources, with explicit environment overrides and a source
//! checkout fallback for development. Discovery creates nothing.
use std::path::{Path, PathBuf};
fn beside_executable(relative: &str) -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let root = executable.parent()?.parent()?;
    let path = root.join(relative);
    path.is_dir().then_some(path)
}
pub fn plugins() -> PathBuf {
    std::env::var_os("BASECRAFT_PLUGINS")
        .map(PathBuf::from)
        .or_else(|| beside_executable("plugins"))
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"))
}
pub fn sdk() -> PathBuf {
    std::env::var_os("BASECRAFT_PYTHONPATH")
        .map(PathBuf::from)
        .or_else(|| beside_executable("python/src"))
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src"))
}
pub fn bundled_python() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let root = executable.parent()?.parent()?;
    bundle_python_at(root)
}

fn bundle_python_at(root: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let portable = root.join("runtime/python.exe");
        if portable.is_file() {
            return Some(portable);
        }
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("build.json")).ok()?).ok()?;
    // Local deployment locations are kept beside, not inside, exported bundles.
    // Older feedback bundles stored this field directly in build.json.
    let local = root.with_file_name(format!("{}.local.json", root.file_name()?.to_str()?));
    let deployment: serde_json::Value = match std::fs::read(local) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => metadata,
        Err(_) => return None,
    };
    let python = Path::new(deployment["python_environment"].as_str()?).join("python");
    python.is_file().then_some(python)
}

pub fn source_python() -> Option<PathBuf> {
    source_python_at(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn source_python_at(root: &Path) -> Option<PathBuf> {
    // Pixi's checkout-local environment also supports a deliberate symlink to
    // a detached environment. Other detached setups use BASECRAFT_PYTHON or PATH.
    let python = root.join(if cfg!(windows) {
        ".pixi/envs/default/python.exe"
    } else {
        ".pixi/envs/default/bin/python"
    });
    python
        .is_file()
        .then(|| python.canonicalize().ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_python_belongs_to_the_selected_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("checkout");
        let other = temp.path().join("other/.pixi/envs/default/bin");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("python"), "").unwrap();
        assert!(source_python_at(&root).is_none());
        let bin = root.join(".pixi/envs/default/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("python"), "").unwrap();
        assert_eq!(source_python_at(&root), Some(bin.join("python")));
    }

    #[test]
    fn detached_source_environment_and_private_bundle_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("release");
        std::fs::create_dir_all(root.join(".pixi/envs")).unwrap();
        let detached = temp.path().join("detached/bin");
        std::fs::create_dir_all(&detached).unwrap();
        std::fs::write(detached.join("python"), "").unwrap();
        std::os::unix::fs::symlink(detached.parent().unwrap(), root.join(".pixi/envs/default"))
            .unwrap();
        assert_eq!(source_python_at(&root), Some(detached.join("python")));
        std::fs::write(root.join("build.json"), "{\"schema_version\":2}").unwrap();
        assert!(bundle_python_at(&root).is_none());
        let receipt = serde_json::json!({"python_environment": detached});
        std::fs::write(temp.path().join("release.local.json"), receipt.to_string()).unwrap();
        assert_eq!(bundle_python_at(&root), Some(detached.join("python")));
        std::fs::remove_file(temp.path().join("release.local.json")).unwrap();
        std::fs::write(root.join("build.json"), receipt.to_string()).unwrap();
        assert_eq!(bundle_python_at(&root), Some(detached.join("python")));
    }
}
