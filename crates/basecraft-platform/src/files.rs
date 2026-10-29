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
