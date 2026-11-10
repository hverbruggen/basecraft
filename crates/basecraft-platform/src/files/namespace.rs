//! Finite, read-only namespace observations. These are not policy grants.
//! Stable aliases and deterministic substitutions are refused. Concurrent hostile
//! mutation, mount changes and in-place content edits are not excluded.
use std::{
    fs::File,
    io,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum NamespaceReadError {
    #[error("unsupported namespace or backend")]
    Unsupported,
    #[error("namespace changed since inspection")]
    Changed,
    #[error("symbolic link in namespace")]
    Redirected,
    #[error("hardlinked file in namespace")]
    Aliased,
    #[error("unexpected filesystem object kind")]
    WrongKind,
    #[error("namespace I/O: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
pub struct ReadNamespace {
    _private: (),
    #[cfg(unix)]
    inner: unix::Namespace,
}
#[derive(Debug)]
pub struct CheckedReadFile(File);
impl Read for CheckedReadFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}
impl CheckedReadFile {
    pub fn metadata(&self) -> io::Result<std::fs::Metadata> {
        self.0.metadata()
    }
}
#[derive(Debug)]
pub struct CheckedAbsence {
    location: PathBuf,
    error: io::Error,
    // Keep the validated prefix alive until the observer consumes the outcome.
    _namespace: ReadNamespace,
}
impl CheckedAbsence {
    /// Logical requested location, not a path authorized for reopening.
    pub fn location(&self) -> &Path {
        &self.location
    }
    pub fn error(&self) -> &io::Error {
        &self.error
    }
    /// Preserve the original bootstrap error, including its OS code and source.
    pub fn into_parts(self) -> (PathBuf, io::Error) {
        (self.location, self.error)
    }
}
#[derive(Debug)]
pub enum CheckedRead {
    File(CheckedReadFile),
    Absent(CheckedAbsence),
}

/// Ordinary Unix backend; only Linux has been qualified by the initial tests.
pub const fn namespace_read_supported() -> bool {
    cfg!(unix)
}

pub fn inspect_read_namespace(path: &Path) -> Result<ReadNamespace, NamespaceReadError> {
    if !namespace_read_supported() {
        return Err(NamespaceReadError::Unsupported);
    }
    #[cfg(unix)]
    {
        unix::inspect(path).map(|inner| ReadNamespace {
            _private: (),
            inner,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(NamespaceReadError::Unsupported)
    }
}
impl ReadNamespace {
    /// Consume once, rechecking the retained lineage before any content open.
    pub fn open_read(self) -> Result<CheckedRead, NamespaceReadError> {
        #[cfg(unix)]
        {
            unix::consume(self)
        }
        #[cfg(not(unix))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::{CheckedAbsence, CheckedRead, CheckedReadFile, NamespaceReadError, ReadNamespace};
    use crate::files::{ObjectIdentity, OrdinaryKind, handle_object_identity};
    use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, Stat, openat, statat};
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStrExt, fs::MetadataExt},
    };
    use std::{
        fs::File,
        io,
        path::{Path, PathBuf},
    };

    #[derive(Debug)]
    struct Directory {
        handle: File,
        identity: ObjectIdentity,
    }
    #[derive(Debug)]
    pub(super) struct Namespace {
        location: PathBuf,
        names: Vec<OsString>,
        directories: Vec<Directory>,
        leaf: Option<ObjectIdentity>,
        missing: Option<io::Error>,
    }
    fn identity(stat: &Stat) -> ObjectIdentity {
        ObjectIdentity::Unix {
            device: stat.st_dev,
            inode: stat.st_ino,
        }
    }
    fn check(stat: &Stat, directory: bool) -> Result<ObjectIdentity, NamespaceReadError> {
        let kind = FileType::from_raw_mode(stat.st_mode);
        if kind == FileType::Symlink {
            return Err(NamespaceReadError::Redirected);
        }
        if kind
            != if directory {
                FileType::Directory
            } else {
                FileType::RegularFile
            }
        {
            return Err(NamespaceReadError::WrongKind);
        }
        if !directory && stat.st_nlink != 1 {
            return Err(NamespaceReadError::Aliased);
        }
        Ok(identity(stat))
    }
    fn lookup(parent: &File, name: &std::ffi::OsStr) -> io::Result<Stat> {
        #[cfg(test)]
        super::tests::record(false, name);
        statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(Into::into)
    }
    fn directory(
        parent: impl std::os::fd::AsFd,
        name: &std::ffi::OsStr,
    ) -> Result<Directory, NamespaceReadError> {
        #[cfg(test)]
        super::tests::record(false, name);
        let handle = File::from(
            openat(
                parent,
                name,
                OFlags::RDONLY
                    | OFlags::DIRECTORY
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        let identity = handle_object_identity(&handle, OrdinaryKind::Directory)?;
        Ok(Directory { handle, identity })
    }
    pub(super) fn inspect(path: &Path) -> Result<Namespace, NamespaceReadError> {
        // Inspect raw spelling: Path::components normalizes interior '.' away.
        let bytes = path.as_os_str().as_bytes();
        if !bytes.starts_with(b"/")
            || bytes.starts_with(b"//")
            || bytes.ends_with(b"/")
            || bytes.contains(&0)
            || bytes
                .split(|b| *b == b'/')
                .skip(1)
                .any(|part| part.is_empty() || part == b"." || part == b"..")
        {
            return Err(NamespaceReadError::Unsupported);
        }
        let names: Vec<_> = path
            .components()
            .skip(1)
            .map(|c| c.as_os_str().to_owned())
            .collect();
        if names.is_empty() {
            return Err(NamespaceReadError::Unsupported);
        }
        let root = directory(CWD, std::ffi::OsStr::new("/"))?;
        let mut ns = Namespace {
            location: path.to_owned(),
            names,
            directories: vec![root],
            leaf: None,
            missing: None,
        };
        for (index, name) in ns.names.iter().enumerate() {
            let parent = &ns.directories[index].handle;
            let stat = match lookup(parent, name) {
                Ok(stat) => stat,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    ns.missing = Some(error);
                    break;
                }
                Err(error) => return Err(error.into()),
            };
            let is_directory = index + 1 < ns.names.len();
            let observed = check(&stat, is_directory)?;
            if is_directory {
                let next = directory(parent, name)?;
                if next.identity != observed {
                    return Err(NamespaceReadError::Changed);
                }
                ns.directories.push(next);
            } else {
                ns.leaf = Some(observed);
            }
        }
        Ok(ns)
    }
    fn recheck(ns: &Namespace) -> Result<(), NamespaceReadError> {
        let root = statat(CWD, "/", AtFlags::SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
        if check(&root, true)? != ns.directories[0].identity {
            return Err(NamespaceReadError::Changed);
        }
        for (index, directory) in ns.directories.iter().enumerate() {
            if handle_object_identity(&directory.handle, OrdinaryKind::Directory)?
                != directory.identity
            {
                return Err(NamespaceReadError::Changed);
            }
            if index > 0 {
                let stat = lookup(&ns.directories[index - 1].handle, &ns.names[index - 1])?;
                if check(&stat, true)? != directory.identity {
                    return Err(NamespaceReadError::Changed);
                }
            }
        }
        Ok(())
    }
    pub(super) fn consume(mut ticket: ReadNamespace) -> Result<CheckedRead, NamespaceReadError> {
        let ns = &mut ticket.inner;
        recheck(ns)?;
        let index = ns.directories.len() - 1;
        let parent = &ns.directories[index].handle;
        let name = &ns.names[index];
        if ns.missing.is_some() {
            match lookup(parent, name) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(stat) => {
                    check(&stat, index + 1 < ns.names.len())?;
                    return Err(NamespaceReadError::Changed);
                }
            }
            recheck(ns)?;
            let error = ns.missing.take().ok_or(NamespaceReadError::Changed)?;
            return Ok(CheckedRead::Absent(CheckedAbsence {
                location: ns.location.clone(),
                error,
                _namespace: ticket,
            }));
        }
        let expected = ns.leaf.as_ref().ok_or(NamespaceReadError::Changed)?;
        if &check(&lookup(parent, name)?, false)? != expected {
            return Err(NamespaceReadError::Changed);
        }
        #[cfg(test)]
        super::tests::record(true, name);
        let file = File::from(
            openat(
                parent,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        if !file.metadata()?.is_file() {
            return Err(NamespaceReadError::WrongKind);
        }
        if file.metadata()?.nlink() != 1 {
            return Err(NamespaceReadError::Aliased);
        }
        if &handle_object_identity(&file, OrdinaryKind::File)? != expected {
            return Err(NamespaceReadError::Changed);
        }
        if &check(&lookup(parent, name)?, false)? != expected {
            return Err(NamespaceReadError::Changed);
        }
        recheck(ns)?;
        Ok(CheckedRead::File(CheckedReadFile(file)))
    }
}

#[cfg(all(test, unix))]
#[path = "namespace_read_tests.rs"]
mod tests;

#[cfg(all(test, not(unix)))]
mod unsupported_tests {
    #[test]
    fn unsupported_backend_refuses_without_inspection() {
        assert!(!super::namespace_read_supported());
        assert!(matches!(
            super::inspect_read_namespace(std::path::Path::new("arbitrary")),
            Err(super::NamespaceReadError::Unsupported)
        ));
    }
}
