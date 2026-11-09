//! Sealed shared-memory buffers for handing bulk data to plugin processes.
//!
//! On Linux a buffer is a `memfd`: written once, then sealed against
//! writes, shrinking and growing, so the receiving process can map it
//! without fearing changes underneath. Windows uses temporary files held
//! read-only with sharing locks that deny writes and deletion. Other platforms
//! fail explicitly until an equivalent immutability guarantee is available.

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
    #[cfg(windows)]
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

    #[cfg(not(any(target_os = "linux", windows)))]
    pub fn new(_name: &str) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "immutable shared buffers require Linux memfd or Windows sharing locks",
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

    #[cfg(not(any(target_os = "linux", windows)))]
    pub fn seal(self) -> io::Result<SharedBuffer> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "immutable shared buffers require Linux memfd or Windows sharing locks",
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
