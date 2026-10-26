//! No-clobber namespace promotion for owned staged artifact directories.
use std::path::Path;
#[cfg(target_os = "linux")]
pub fn rename_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub fn rename_noreplace(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic collection publication requires rename-noreplace support",
    ))
}
