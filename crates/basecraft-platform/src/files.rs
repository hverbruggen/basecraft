//! Ordinary read-only evidence files, without blocking on Unix special files.
use std::{fs::File, io, path::Path};

pub fn open_regular_read(path: &Path) -> io::Result<File> {
    // Follow existing regular-file symlinks, while rejecting known special
    // files before opening them (including devices with open-time effects).
    if !std::fs::metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "evidence is not a regular file",
        ));
    }
    #[cfg(unix)]
    let file = File::from(rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?);
    #[cfg(not(unix))]
    let file = File::open(path)?;
    // Recheck the actual opened handle: a regular path may have been replaced
    // since metadata. NONBLOCK prevents a substituted Unix FIFO from hanging.
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "opened evidence is not a regular file",
        ));
    }
    Ok(file)
}

/// Stable identity and hard-link count from an opened file, never synthetic IDs.
pub fn handle_identity(file: &File) -> io::Result<(u64, u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata()?;
        Ok((m.dev(), m.ino(), m.nlink()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        // SAFETY: File retains a valid handle and information points to initialized storage.
        unsafe {
            let mut information: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
            if GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok((
                u64::from(information.dwVolumeSerialNumber),
                (u64::from(information.nFileIndexHigh) << 32)
                    | u64::from(information.nFileIndexLow),
                u64::from(information.nNumberOfLinks),
            ))
        }
    }
}
pub fn link_count(path: &Path) -> io::Result<u64> {
    Ok(handle_identity(&open_regular_read(path)?)?.2)
}
pub fn identity(path: &Path) -> io::Result<(u64, u64)> {
    let value = handle_identity(&open_regular_read(path)?)?;
    Ok((value.0, value.1))
}

/// Sync a retained file before publication. Windows `FlushFileBuffers` requires
/// a writable handle; Unix retains its existing read-only fsync behavior.
pub fn sync_file(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    #[cfg(not(windows))]
    let file = File::open(path)?;
    file.sync_all()
}
/// Unix directory fsync has no supported Windows equivalent. Windows promotion
/// uses `MoveFileExW` with `WRITE_THROUGH`; do not pretend to flush a directory handle.
pub fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(windows)]
    {
        if path.is_dir() {
            Ok(())
        } else {
            Err(io::Error::other("publication parent is not a directory"))
        }
    }
}

#[cfg(test)]
mod identity_tests {
    #[test]
    fn identity_detects_hardlinks_and_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        std::fs::write(&path, b"same bytes").unwrap();
        let before = super::identity(&path).unwrap();
        let alias = dir.path().join("alias");
        std::fs::hard_link(&path, &alias).unwrap();
        assert_eq!(before, super::identity(&alias).unwrap());
        assert_eq!(super::link_count(&path).unwrap(), 2);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"same bytes").unwrap();
        assert_ne!(before, super::identity(&path).unwrap());
    }
}
