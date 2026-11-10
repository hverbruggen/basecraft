//! No-clobber namespace promotion for owned staged artifact directories.
use std::path::Path;
// macOS: rustix maps NOREPLACE to renameatx_np(RENAME_EXCL).
#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod tests {
    #[test]
    fn promotion_preserves_a_competing_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("staged");
        let destination = root.path().join("published");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("data"), b"new").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("data"), b"existing").unwrap();
        assert!(super::rename_noreplace(&source, &destination).is_err());
        assert_eq!(
            std::fs::read(destination.join("data")).unwrap(),
            b"existing"
        );
        assert_eq!(std::fs::read(source.join("data")).unwrap(), b"new");
        let free = root.path().join("new destination");
        super::rename_noreplace(&source, &free).unwrap();
        assert_eq!(std::fs::read(free.join("data")).unwrap(), b"new");
        assert!(!source.exists());
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn rename_noreplace(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic collection publication requires rename-noreplace support",
    ))
}

#[cfg(windows)]
pub fn rename_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let wide = |path: &Path| -> std::io::Result<Vec<u16>> {
        let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains NUL",
            ));
        }
        value.push(0);
        Ok(value)
    };
    let source = wide(source)?;
    let destination = wide(destination)?;
    // SAFETY: both strings are NUL-terminated and live throughout the call.
    // No REPLACE_EXISTING or COPY_ALLOWED: never overwrite or cross filesystems.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
