//! Sealed shared-memory buffers for handing bulk data to plugin processes.
//!
//! On Linux a buffer is a `memfd`: written once, then sealed against
//! writes, shrinking and growing, so the receiving process can map it
//! without fearing changes underneath. Windows uses temporary files held
//! read-only with sharing locks that deny writes and deletion. macOS has
//! neither seals nor mandatory locks. Its temporary snapshot is reopened
//! read-only and unlinked before the writer is closed. This requires trusted,
//! cooperative same-user processes: it does not revoke a writable handle
//! obtained before unlink, and is not kernel-enforced Linux-style sealing.
//! Other platforms fail explicitly.

use std::fs::File;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

/// A filled, sealed, read-only shared buffer.
pub struct SharedBuffer {
    fd: File,
    len: u64,
    #[cfg(windows)]
    path: tempfile::TempPath,
}

/// A buffer being written. Call [`SharedWriter::seal`] when done.
pub struct SharedWriter {
    file: File,
    len: u64,
    #[cfg(any(windows, target_os = "macos"))]
    path: tempfile::TempPath,
}

impl SharedWriter {
    #[cfg(target_os = "linux")]
    pub fn new(name: &str) -> io::Result<Self> {
        use rustix::fs::{MemfdFlags, memfd_create};
        let fd = memfd_create(name, MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING)?;
        Ok(Self {
            file: File::from(fd),
            len: 0,
        })
    }

    #[cfg(target_os = "macos")]
    pub fn new(name: &str) -> io::Result<Self> {
        let (file, path) = tempfile::Builder::new()
            .prefix(&format!("basecraft-{name}-"))
            .tempfile()?
            .into_parts();
        Ok(Self { file, len: 0, path })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub fn new(_name: &str) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "immutable shared buffers are implemented for Linux, macOS and Windows",
        ))
    }

    #[cfg(windows)]
    pub fn new(_name: &str) -> io::Result<Self> {
        let (file, path) = tempfile::NamedTempFile::new()?.into_parts();
        Ok(Self { file, len: 0, path })
    }

    #[cfg(windows)]
    pub fn seal(mut self) -> io::Result<SharedBuffer> {
        use std::os::windows::fs::OpenOptionsExt;
        self.file.flush()?;
        self.file.sync_all()?;
        // Reopen read-only while retaining the path; deny writes and deletion
        // for the complete snapshot lifetime, including plugin mmap lifetime.
        let identity = crate::files::handle_identity(&self.file)?;
        drop(self.file);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&self.path)?;
        if crate::files::handle_identity(&file)? != identity {
            return Err(io::Error::other("snapshot file changed during sealing"));
        }
        Ok(SharedBuffer {
            fd: file,
            len: self.len,
            path: self.path,
        })
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The underlying file, unsealed.
    pub fn into_file(self) -> File {
        self.file
    }

    /// Seal against all further changes and return the read-only buffer.
    #[cfg(target_os = "linux")]
    pub fn seal(mut self) -> io::Result<SharedBuffer> {
        use rustix::fs::{SealFlags, fcntl_add_seals};
        self.file.flush()?;
        let fd = OwnedFd::from(self.file);
        fcntl_add_seals(
            &fd,
            SealFlags::WRITE | SealFlags::SHRINK | SealFlags::GROW | SealFlags::SEAL,
        )?;
        Ok(SharedBuffer {
            fd: File::from(fd),
            len: self.len,
        })
    }

    #[cfg(target_os = "macos")]
    pub fn seal(mut self) -> io::Result<SharedBuffer> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        self.file.flush()?;
        self.file.sync_all()?;
        let written = self.file.metadata()?;
        self.file
            .set_permissions(std::fs::Permissions::from_mode(0o400))?;
        let file = File::open(&self.path)?;
        let opened = file.metadata()?;
        if (opened.dev(), opened.ino()) != (written.dev(), written.ino())
            || opened.len() != self.len
        {
            return Err(io::Error::other("snapshot file changed during sealing"));
        }
        // Remove the name before dropping our writer. Under the trusted-plugin
        // contract no other process retains a writable handle to this file.
        self.path.close()?;
        drop(self.file);
        Ok(SharedBuffer {
            fd: file,
            len: self.len,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    pub fn seal(self) -> io::Result<SharedBuffer> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "immutable shared buffers are implemented for Linux, macOS and Windows",
        ))
    }
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.file.write(buf)?;
        self.len += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl SharedBuffer {
    #[cfg(unix)]
    pub fn transport_handle(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
    #[cfg(windows)]
    pub fn transport_handle(&self) -> &std::path::Path {
        &self.path
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Wrap a buffer received from another process.
    #[cfg(unix)]
    pub fn from_fd(fd: OwnedFd) -> io::Result<Self> {
        let len = File::from(fd.try_clone()?).metadata()?.len();
        Ok(Self {
            fd: File::from(fd),
            len,
        })
    }

    /// Map the buffer read-only.
    pub fn map(&self) -> io::Result<memmap2::Mmap> {
        // SAFETY: buffers we create are sealed against writes and size
        // changes before anyone maps them, so the mapping can't change or
        // shrink underneath us. A buffer received from a plugin may not be
        // sealed; plugins are trusted not to resize what they send, and the
        // protocol requires them to seal.
        // macOS additionally relies on cooperative same-user processes not
        // retaining a writable handle before the snapshot is unlinked.
        unsafe { memmap2::Mmap::map(&self.fd) }
    }
}

#[cfg(unix)]
impl AsFd for SharedBuffer {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn write_seal_map() {
        let mut w = SharedWriter::new("test").unwrap();
        w.write_all(b"ACGT").unwrap();
        let buf = w.seal().unwrap();
        assert_eq!(buf.len(), 4);
        assert_eq!(&buf.map().unwrap()[..], b"ACGT");
        // Sealed: a write through a duplicate descriptor fails.
        let mut f = buf.fd.try_clone().unwrap();
        assert!(f.write_all(b"X").is_err());
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    #[test]
    fn snapshot_denies_write_delete_and_removes_after_drop() {
        let mut writer = SharedWriter::new("immutable").unwrap();
        writer.write_all(b"ACGT").unwrap();
        let buffer = writer.seal().unwrap();
        let path = buffer.transport_handle().to_owned();
        assert_eq!(&buffer.map().unwrap()[..], b"ACGT");
        assert_eq!(std::fs::read(&path).unwrap(), b"ACGT");
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        drop(buffer);
        assert!(!path.exists());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn sealed_snapshot_is_read_only_and_unnamed() {
        let mut writer = SharedWriter::new("immutable").unwrap();
        writer.write_all(b"ACGT").unwrap();
        let path = writer.path.to_path_buf();
        let buffer = writer.seal().unwrap();
        assert_eq!(buffer.len(), 4);
        assert_eq!(&buffer.map().unwrap()[..], b"ACGT");
        // No name to reopen, and the descriptor handed to plugins can't write.
        assert!(!path.exists());
        let mut duplicate = buffer.fd.try_clone().unwrap();
        assert!(duplicate.write_all(b"X").is_err());
        assert_eq!(
            buffer.fd.metadata().unwrap().permissions().mode() & 0o777,
            0o400
        );
        assert_eq!(&buffer.map().unwrap()[..], b"ACGT");
    }
}
