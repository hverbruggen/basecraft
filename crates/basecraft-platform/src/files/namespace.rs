//! Finite namespace observations and consuming mechanical transitions. These are not policy grants.
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
    #[cfg(target_os = "linux")]
    inner: linux::Namespace,
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

/// Ordinary Linux backend. All other targets refuse before filesystem probes.
pub const fn namespace_read_supported() -> bool {
    cfg!(target_os = "linux")
}

pub fn inspect_read_namespace(path: &Path) -> Result<ReadNamespace, NamespaceReadError> {
    if !namespace_read_supported() {
        return Err(NamespaceReadError::Unsupported);
    }
    #[cfg(target_os = "linux")]
    {
        linux::inspect(path).map(|inner| ReadNamespace {
            _private: (),
            inner,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(NamespaceReadError::Unsupported)
    }
}
impl ReadNamespace {
    /// Consume once, rechecking the retained lineage before any content open.
    pub fn open_read(self) -> Result<CheckedRead, NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            linux::consume(self)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
}

/// Metadata from a no-follow observation, never a content-capable target handle.
#[derive(Debug)]
pub struct TargetMetadata {
    identity: crate::files::ObjectIdentity,
    kind: crate::files::OrdinaryKind,
    links: u64,
}
impl TargetMetadata {
    pub fn identity(&self) -> &crate::files::ObjectIdentity {
        &self.identity
    }
    pub fn kind(&self) -> crate::files::OrdinaryKind {
        self.kind
    }
    pub fn links(&self) -> u64 {
        self.links
    }
}

/// Retains the complete lineage. Absence is only accepted at the final leaf.
#[derive(Debug)]
pub struct TargetNamespace {
    _private: (),
    #[cfg(target_os = "linux")]
    inner: linux::Namespace,
}

/// Structural evidence of an actual directory move, not publication consent.
/// No caller-supplied identity, success flag or fresh path inspection can create it.
/// ```compile_fail
/// use basecraft_platform::files::PromotedDirectory;
/// let forged = PromotedDirectory { destination: todo!() };
/// ```
/// ```compile_fail
/// use basecraft_platform::files::PromotedDirectory;
/// fn duplicate(moved: PromotedDirectory) { let copy = moved.clone(); }
/// ```
#[derive(Debug)]
pub struct PromotedDirectory {
    destination: TargetNamespace,
}
impl PromotedDirectory {
    pub fn destination(&self) -> &TargetNamespace {
        &self.destination
    }
    pub fn into_destination(self) -> TargetNamespace {
        self.destination
    }
}

/// Both input observations are consumed on every outcome. Neither failure
/// permits retry with the old tickets. A reported move is never rolled back.
#[derive(Debug, thiserror::Error)]
pub enum DirectoryPromotionError {
    #[error("no reported rename success: {0}")]
    NotMoved(#[source] NamespaceReadError),
    #[error("directory moved, but post-check failed: {0}")]
    MovedWithPostcheckError(#[source] NamespaceReadError),
}

/// Existing evidence state directories retain their mode. A private reservation
/// directory can be repaired to 0700 by a separately authorized step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockDirectoryMode {
    Preserve,
    Private,
}

/// Exactly the next predetermined effect; the caller authorizes each separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockStep {
    CreateDirectory,
    SetPermissions,
    OpenFile,
}

/// A consuming step cannot be replayed, nor select an arbitrary child.
/// ```compile_fail
/// use basecraft_platform::files::LockNamespace;
/// fn replay(ticket: LockNamespace) {
///     let updated = ticket.create_next_directory();
///     let replay = ticket.create_next_directory();
/// }
/// ```
#[derive(Debug)]
pub struct LockNamespace {
    _private: (),
    #[cfg(target_os = "linux")]
    inner: linux::Lock,
}

/// Owns the sole writable lock handle and retained namespace. No clone, path
/// reopen, unlink or implicit advisory lock. Borrow its descriptor for locking;
/// recheck after acquisition (especially after blocking) before using the guard.
#[derive(Debug)]
pub struct OpenedLock {
    _private: (),
    #[cfg(target_os = "linux")]
    inner: linux::Opened,
}

pub const fn namespace_lock_supported() -> bool {
    cfg!(target_os = "linux")
}

pub fn inspect_target_namespace(path: &Path) -> Result<TargetNamespace, NamespaceReadError> {
    #[cfg(target_os = "linux")]
    {
        linux::inspect_target(path).map(|inner| TargetNamespace {
            _private: (),
            inner,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(NamespaceReadError::Unsupported)
    }
}
impl TargetNamespace {
    /// Move this observed directory to the observed absent final leaf, once.
    /// Existing retained parents and fixed endpoint names are required. This
    /// performs no sync, cleanup, locking or publication-policy authorization.
    /// Concurrent hostile namespace mutation and inode reuse are not excluded.
    /// ```compile_fail
    /// use basecraft_platform::files::TargetNamespace;
    /// fn replay(source: TargetNamespace, destination: TargetNamespace) {
    ///     let result = source.promote_directory_to(destination);
    ///     let replay = source.promote_directory_to(destination);
    /// }
    /// ```
    /// ```compile_fail
    /// use basecraft_platform::files::TargetNamespace;
    /// fn reuse_destination(source: TargetNamespace, destination: TargetNamespace) {
    ///     let result = source.promote_directory_to(destination);
    ///     destination.recheck();
    /// }
    /// ```
    pub fn promote_directory_to(
        self,
        destination: TargetNamespace,
    ) -> Result<PromotedDirectory, DirectoryPromotionError> {
        #[cfg(target_os = "linux")]
        {
            linux::promote(self.inner, destination.inner).map(|inner| PromotedDirectory {
                destination: TargetNamespace {
                    _private: (),
                    inner,
                },
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = destination;
            Err(DirectoryPromotionError::NotMoved(
                NamespaceReadError::Unsupported,
            ))
        }
    }

    /// Consume a checked absence without reconstructing its original IO error.
    /// A present observation is returned intact for the caller to handle.
    pub fn into_absence_error(self) -> Result<io::Error, Box<Self>> {
        #[cfg(target_os = "linux")]
        {
            let mut observation = self;
            match observation.inner.missing.take() {
                Some(error) => Ok(error),
                None => Err(Box::new(observation)),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(Box::new(self))
        }
    }
    pub fn metadata(&self) -> Option<&TargetMetadata> {
        #[cfg(target_os = "linux")]
        {
            self.inner.metadata.as_ref()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
    /// Original missing-leaf error; never reconstructed from kind or text.
    pub fn absence_error(&self) -> Option<&io::Error> {
        #[cfg(target_os = "linux")]
        {
            self.inner.missing.as_ref()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
    pub fn parent_identity(&self) -> Result<&crate::files::ObjectIdentity, NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.parent_identity()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
    pub fn recheck(&self) -> Result<(), NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            linux::recheck_leaf(&self.inner)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
}

/// The path must be supplied by the authorized finite owner, not a wire caller.
/// This function validates structure only; it does not grant permission.
pub fn inspect_lock_namespace(
    path: &Path,
    mode: LockDirectoryMode,
) -> Result<LockNamespace, NamespaceReadError> {
    #[cfg(target_os = "linux")]
    {
        linux::inspect_lock(path, mode).map(|inner| LockNamespace {
            _private: (),
            inner,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, mode);
        Err(NamespaceReadError::Unsupported)
    }
}
impl LockNamespace {
    pub fn next_step(&self) -> Result<LockStep, NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            Ok(self.inner.next_step())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
    pub fn recheck(&self) -> Result<(), NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.recheck()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
    /// At most one mkdir, followed only by metadata checks. On failure, already
    /// created directories remain; no retry/adoption of a raced EEXIST.
    pub fn create_next_directory(self) -> Result<Self, NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.create_directory().map(|inner| Self {
                _private: (),
                inner,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
    /// One fchmod on the predetermined owned directory, never a path chmod.
    pub fn set_next_permissions(self) -> Result<Self, NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.set_permissions().map(|inner| Self {
                _private: (),
                inner,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
    /// One RDWR open, creating exclusively only after a checked missing leaf.
    pub fn open_file(self) -> Result<OpenedLock, NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.open().map(|inner| OpenedLock {
                _private: (),
                inner,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
}
impl OpenedLock {
    /// Forward to fs4 on the one retained file. The owner must authorize this
    /// effect and recheck the namespace after success, especially after a wait.
    pub fn try_lock_exclusive(&self) -> Result<bool, NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            fs4::fs_std::FileExt::try_lock_exclusive(&self.inner.file).map_err(Into::into)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
    pub fn lock_exclusive(&self) -> Result<(), NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            fs4::fs_std::FileExt::lock_exclusive(&self.inner.file).map_err(Into::into)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
    pub fn recheck(&self) -> Result<(), NamespaceReadError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.recheck()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(NamespaceReadError::Unsupported)
        }
    }
}
#[cfg(target_os = "linux")]
impl std::os::fd::AsFd for OpenedLock {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.inner.file)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{
        CheckedAbsence, CheckedRead, CheckedReadFile, LockDirectoryMode, LockStep,
        NamespaceReadError, ReadNamespace, TargetMetadata,
    };
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
        pub(super) missing: Option<io::Error>,
        pub(super) metadata: Option<TargetMetadata>,
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
        inspect_kind(path, false)
    }
    fn inspect_kind(path: &Path, target: bool) -> Result<Namespace, NamespaceReadError> {
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
            metadata: None,
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
            let leaf_directory = target
                && !is_directory
                && FileType::from_raw_mode(stat.st_mode) == FileType::Directory;
            let observed = check(&stat, is_directory || leaf_directory)?;
            if is_directory {
                let next = directory(parent, name)?;
                if next.identity != observed {
                    return Err(NamespaceReadError::Changed);
                }
                ns.directories.push(next);
            } else {
                ns.metadata = Some(TargetMetadata {
                    identity: observed.clone(),
                    kind: if leaf_directory {
                        OrdinaryKind::Directory
                    } else {
                        OrdinaryKind::File
                    },
                    links: stat.st_nlink,
                });
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
    impl Namespace {
        pub(super) fn parent_identity(&self) -> Result<&ObjectIdentity, NamespaceReadError> {
            self.directories
                .last()
                .map(|d| &d.identity)
                .ok_or(NamespaceReadError::Changed)
        }
    }

    pub(super) fn inspect_target(path: &Path) -> Result<Namespace, NamespaceReadError> {
        let mut ns = inspect_kind(path, true)?;
        if ns.directories.len() != ns.names.len() {
            return Err(ns
                .missing
                .take()
                .map_or(NamespaceReadError::Changed, NamespaceReadError::Io));
        }
        recheck_leaf(&ns)?;
        Ok(ns)
    }

    // Shared metadata-only association check, including checked absence. Never
    // call link_count/object_identity: those legacy helpers open file contents.
    pub(super) fn recheck_leaf(ns: &Namespace) -> Result<(), NamespaceReadError> {
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
        } else {
            let metadata = ns.metadata.as_ref().ok_or(NamespaceReadError::Changed)?;
            let observed = check(
                &lookup(parent, name)?,
                metadata.kind == OrdinaryKind::Directory,
            )?;
            if observed != metadata.identity {
                return Err(NamespaceReadError::Changed);
            }
        }
        recheck(ns)
    }

    pub(super) fn promote(
        source: Namespace,
        mut destination: Namespace,
    ) -> Result<Namespace, super::DirectoryPromotionError> {
        use super::DirectoryPromotionError::{MovedWithPostcheckError, NotMoved};
        #[cfg(test)]
        super::promotion_tests::seam(super::promotion_tests::Seam::BeforePrecheck);
        let precheck = || {
            if source.metadata.as_ref().map(|m| m.kind) != Some(OrdinaryKind::Directory) {
                return Err(NamespaceReadError::WrongKind);
            }
            if destination.missing.is_none() || destination.metadata.is_some() {
                return Err(NamespaceReadError::Changed);
            }
            recheck_leaf(&source)?;
            recheck_leaf(&destination)
        };
        precheck().map_err(NotMoved)?;
        let expected = source
            .metadata
            .ok_or(NotMoved(NamespaceReadError::Changed))?;
        let source_index = source.names.len() - 1;
        let destination_index = destination.names.len() - 1;
        #[cfg(test)]
        super::promotion_tests::seam(super::promotion_tests::Seam::BeforeRename);
        crate::atomic::renameat_noreplace(
            &source.directories[source_index].handle,
            &source.names[source_index],
            &destination.directories[destination_index].handle,
            &destination.names[destination_index],
        )
        .map_err(|error| NotMoved(NamespaceReadError::Io(error)))?;
        // Success boundary: every subsequent failure is irrevocably a moved
        // outcome. No fallible action can return NotMoved from here onward.
        #[cfg(test)]
        super::promotion_tests::seam(super::promotion_tests::Seam::AfterRename);
        destination.leaf = Some(expected.identity.clone());
        destination.metadata = Some(expected);
        destination.missing = None;
        recheck_leaf(&destination).map_err(MovedWithPostcheckError)?;
        Ok(destination)
    }

    #[derive(Debug)]
    struct OwnedDirectory {
        index: usize,
        mode: u32,
    }
    #[derive(Debug)]
    pub(super) struct Lock {
        namespace: Namespace,
        owned: Vec<OwnedDirectory>,
        pending_mode: Option<usize>,
    }
    #[derive(Debug)]
    pub(super) struct Opened {
        lock: Lock,
        pub(super) file: File,
    }
    fn owner(uid: u32) -> Result<(), NamespaceReadError> {
        if uid != rustix::process::geteuid().as_raw() {
            return Err(NamespaceReadError::Unsupported);
        }
        Ok(())
    }
    fn directory_mode(directory: &Directory) -> Result<u32, NamespaceReadError> {
        let metadata = directory.handle.metadata()?;
        owner(metadata.uid())?;
        Ok(metadata.mode() & 0o7777)
    }
    fn exclusive_error(error: rustix::io::Errno) -> NamespaceReadError {
        if error == rustix::io::Errno::EXIST {
            NamespaceReadError::Changed
        } else {
            NamespaceReadError::Io(error.into())
        }
    }
    pub(super) fn inspect_lock(
        path: &Path,
        mode: LockDirectoryMode,
    ) -> Result<Lock, NamespaceReadError> {
        let namespace = inspect(path)?;
        let mut lock = Lock {
            namespace,
            owned: Vec::new(),
            pending_mode: None,
        };
        let ns = &lock.namespace;
        // Only the exact service parent must be user-owned. System ancestors
        // and the sticky temporary container are neither repaired nor adopted.
        if ns.directories.len() == ns.names.len() {
            lock.remember_directory(ns.directories.len() - 1, mode == LockDirectoryMode::Private)?;
        }
        lock.recheck()?;
        Ok(lock)
    }
    impl Lock {
        fn remember_directory(
            &mut self,
            index: usize,
            repair: bool,
        ) -> Result<(), NamespaceReadError> {
            let mode = directory_mode(&self.namespace.directories[index])?;
            self.owned.push(OwnedDirectory { index, mode });
            if repair && mode != 0o700 {
                self.pending_mode = Some(index);
            }
            Ok(())
        }
        pub(super) fn next_step(&self) -> LockStep {
            if self.pending_mode.is_some() {
                LockStep::SetPermissions
            } else if self.namespace.directories.len() < self.namespace.names.len() {
                LockStep::CreateDirectory
            } else {
                LockStep::OpenFile
            }
        }
        pub(super) fn recheck(&self) -> Result<(), NamespaceReadError> {
            recheck_leaf(&self.namespace)?;
            for owned in &self.owned {
                if directory_mode(&self.namespace.directories[owned.index])? != owned.mode {
                    return Err(NamespaceReadError::Changed);
                }
            }
            if self.namespace.leaf.is_some() {
                let ns = &self.namespace;
                let index = ns.directories.len() - 1;
                owner(lookup(&ns.directories[index].handle, &ns.names[index])?.st_uid)?;
            }
            Ok(())
        }
        pub(super) fn create_directory(mut self) -> Result<Self, NamespaceReadError> {
            if self.next_step() != LockStep::CreateDirectory {
                return Err(NamespaceReadError::Unsupported);
            }
            self.recheck()?;
            let ns = &mut self.namespace;
            let index = ns.directories.len() - 1;
            let parent = &ns.directories[index].handle;
            let name = &ns.names[index];
            #[cfg(test)]
            super::lock_tests::effect(LockStep::CreateDirectory);
            rustix::fs::mkdirat(parent, name, Mode::RWXU).map_err(exclusive_error)?;
            let stat = lookup(parent, name)?;
            let expected = check(&stat, true)?;
            owner(stat.st_uid)?;
            let next = directory(parent, name)?;
            if next.identity != expected {
                return Err(NamespaceReadError::Changed);
            }
            ns.directories.push(next);
            // The originally absent suffix was predetermined. Do not adopt an
            // entry that appeared inside the directory, even if ordinary.
            let index = ns.directories.len() - 1;
            match lookup(&ns.directories[index].handle, &ns.names[index]) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => ns.missing = Some(error),
                Err(error) => return Err(error.into()),
                Ok(_) => return Err(NamespaceReadError::Changed),
            }
            self.remember_directory(index, true)?;
            self.recheck()?;
            Ok(self)
        }
        pub(super) fn set_permissions(mut self) -> Result<Self, NamespaceReadError> {
            if self.next_step() != LockStep::SetPermissions {
                return Err(NamespaceReadError::Unsupported);
            }
            self.recheck()?;
            let index = self.pending_mode.ok_or(NamespaceReadError::Changed)?;
            #[cfg(test)]
            super::lock_tests::effect(LockStep::SetPermissions);
            rustix::fs::fchmod(&self.namespace.directories[index].handle, Mode::RWXU)
                .map_err(io::Error::from)?;
            let mode = directory_mode(&self.namespace.directories[index])?;
            if mode != 0o700 {
                return Err(NamespaceReadError::Changed);
            }
            self.owned
                .iter_mut()
                .find(|owned| owned.index == index)
                .ok_or(NamespaceReadError::Changed)?
                .mode = mode;
            self.pending_mode = None;
            self.recheck()?;
            Ok(self)
        }
        pub(super) fn open(mut self) -> Result<Opened, NamespaceReadError> {
            if self.next_step() != LockStep::OpenFile {
                return Err(NamespaceReadError::Unsupported);
            }
            self.recheck()?;
            let ns = &mut self.namespace;
            let index = ns.directories.len() - 1;
            let parent = &ns.directories[index].handle;
            let name = &ns.names[index];
            let creating = ns.missing.is_some();
            let flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
            #[cfg(test)]
            super::lock_tests::effect(LockStep::OpenFile);
            let file = File::from(
                openat(
                    parent,
                    name,
                    if creating {
                        flags | OFlags::CREATE | OFlags::EXCL
                    } else {
                        flags
                    },
                    if creating {
                        Mode::RUSR | Mode::WUSR
                    } else {
                        Mode::empty()
                    },
                )
                .map_err(exclusive_error)?,
            );
            #[cfg(test)]
            super::lock_tests::opened();
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                return Err(NamespaceReadError::WrongKind);
            }
            if metadata.nlink() != 1 {
                return Err(NamespaceReadError::Aliased);
            }
            owner(metadata.uid())?;
            let observed = handle_object_identity(&file, OrdinaryKind::File)?;
            if !creating && ns.leaf.as_ref() != Some(&observed) {
                return Err(NamespaceReadError::Changed);
            }
            ns.leaf = Some(observed.clone());
            ns.metadata = Some(TargetMetadata {
                identity: observed,
                kind: OrdinaryKind::File,
                links: metadata.nlink(),
            });
            ns.missing = None;
            let opened = Opened { lock: self, file };
            opened.recheck()?;
            Ok(opened)
        }
    }
    impl Opened {
        pub(super) fn recheck(&self) -> Result<(), NamespaceReadError> {
            self.lock.recheck()?;
            let metadata = self.file.metadata()?;
            if !metadata.is_file() {
                return Err(NamespaceReadError::WrongKind);
            }
            if metadata.nlink() != 1 {
                return Err(NamespaceReadError::Aliased);
            }
            owner(metadata.uid())?;
            if self.lock.namespace.leaf.as_ref()
                != Some(&handle_object_identity(&self.file, OrdinaryKind::File)?)
            {
                return Err(NamespaceReadError::Changed);
            }
            Ok(())
        }
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    include!("namespace_read_tests.rs");
    pub(super) fn lock_trace() -> Vec<(bool, std::ffi::OsString)> {
        trace()
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod unsupported_tests {
    #[test]
    fn unsupported_backend_refuses_existing_missing_and_invalid_paths() {
        assert!(!super::namespace_read_supported());
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("preferences.toml");
        std::fs::write(&file, "schema_version = 1\n").unwrap();
        for path in [
            root.path(),
            file.as_path(),
            &root.path().join("missing/leaf"),
            std::path::Path::new("relative"),
            std::path::Path::new(""),
        ] {
            assert!(matches!(
                super::inspect_read_namespace(path),
                Err(super::NamespaceReadError::Unsupported)
            ));
        }
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "schema_version = 1\n"
        );
        assert!(!root.path().join("missing").exists());
    }

    #[test]
    fn unsupported_lock_and_target_interfaces_never_bootstrap() {
        assert!(!super::namespace_lock_supported());
        for path in [
            std::path::Path::new("/ordinary/lock"),
            std::path::Path::new("relative"),
        ] {
            assert!(matches!(
                super::inspect_target_namespace(path),
                Err(super::NamespaceReadError::Unsupported)
            ));
            assert!(matches!(
                super::inspect_lock_namespace(path, super::LockDirectoryMode::Private),
                Err(super::NamespaceReadError::Unsupported)
            ));
        }
        let ticket = super::LockNamespace { _private: () };
        assert!(matches!(
            ticket.next_step(),
            Err(super::NamespaceReadError::Unsupported)
        ));
        assert!(matches!(
            ticket.recheck(),
            Err(super::NamespaceReadError::Unsupported)
        ));
        assert!(matches!(
            ticket.create_next_directory(),
            Err(super::NamespaceReadError::Unsupported)
        ));
        assert!(matches!(
            (super::LockNamespace { _private: () }).set_next_permissions(),
            Err(super::NamespaceReadError::Unsupported)
        ));
        assert!(matches!(
            (super::LockNamespace { _private: () }).open_file(),
            Err(super::NamespaceReadError::Unsupported)
        ));
        assert!(matches!(
            (super::OpenedLock { _private: () }).recheck(),
            Err(super::NamespaceReadError::Unsupported)
        ));
        let target = super::TargetNamespace { _private: () };
        assert!(matches!(
            target.recheck(),
            Err(super::NamespaceReadError::Unsupported)
        ));
        assert!(matches!(
            target.parent_identity(),
            Err(super::NamespaceReadError::Unsupported)
        ));
        assert!(target.metadata().is_none());
        assert!(target.absence_error().is_none());
        assert!(matches!(
            target.promote_directory_to(super::TargetNamespace { _private: () }),
            Err(super::DirectoryPromotionError::NotMoved(
                super::NamespaceReadError::Unsupported
            ))
        ));
    }

    #[test]
    fn unsupported_consumption_cannot_return_a_file_or_absence() {
        // Only this descendant test module can construct the private sentinel;
        // public callers cannot construct or obtain a ticket on this backend.
        let ticket = super::ReadNamespace { _private: () };
        assert!(matches!(
            ticket.open_read(),
            Err(super::NamespaceReadError::Unsupported)
        ));
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "namespace_lock_tests.rs"]
mod lock_tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "namespace_promotion_tests.rs"]
mod promotion_tests;
