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
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("build.json")).ok()?).ok()?;
    let python = Path::new(metadata["python_environment"].as_str()?).join("python");
    python.is_file().then_some(python)
}
