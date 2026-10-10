//! Sealed shared-memory buffers for handing bulk data to plugin processes.
//!
//! On Linux a buffer is a `memfd`: written once, then sealed against
//! writes, shrinking and growing, so the receiving process can map it
//! without fearing changes underneath. Other platforms will get their own
//! implementation behind the same type.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

/// A filled, sealed, read-only shared buffer.
pub struct SharedBuffer {
    fd: OwnedFd,
    len: u64,
}

/// A buffer being written. Call [`SharedWriter::seal`] when done.
pub struct SharedWriter {
    file: File,
    len: u64,
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

    #[cfg(not(target_os = "linux"))]
    pub fn new(_name: &str) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "shared buffers need Linux memfd for now",
        ))
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
        Ok(SharedBuffer { fd, len: self.len })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn seal(self) -> io::Result<SharedBuffer> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "shared buffers need Linux memfd for now",
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
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Wrap a buffer received from another process.
    pub fn from_fd(fd: OwnedFd) -> io::Result<Self> {
        let len = File::from(fd.try_clone()?).metadata()?.len();
        Ok(Self { fd, len })
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
        let mut f = File::from(buf.fd.try_clone().unwrap());
        assert!(f.write_all(b"X").is_err());
    }
}
