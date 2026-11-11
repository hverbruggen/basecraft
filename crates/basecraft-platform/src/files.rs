//! Ordinary read-only evidence files, without blocking on Unix special files.
use std::{fs::File, io, path::Path};


/// An observed filesystem object, not a permanent or globally unique ID.
/// IDs may be reused after deletion. All Windows identifier bits are retained.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub enum ObjectIdentity {
    Unix {
        device: u64,
        inode: u64,
    },
    Windows {
        volume_serial: u64,
        file_id: [u8; 16],
    },
}

/// The ordinary object required by an identity observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinaryKind {
    File,
    Directory,
}

fn check_ordinary(metadata: &std::fs::Metadata, expected: OrdinaryKind) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "object is a reparse point",
            ));
        }
    }
    let matches = match expected {
        OrdinaryKind::File => metadata.is_file(),
        OrdinaryKind::Directory => metadata.is_dir(),
    };
    if !matches || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not the expected ordinary file or directory",
        ));
    }
    Ok(())
}

/// Observe an ordinary file/directory without following a leaf link or reparse
/// point. Ancestors may be followed; this is not descriptor-relative confinement.
/// The opened handle is checked again and closed before returning. OS failures
/// are preserved, with no metadata-derived or narrower-identity fallback.
pub fn object_identity(path: &Path, expected: OrdinaryKind) -> io::Result<ObjectIdentity> {
    // Reject known special files before opening them (devices can have effects).
    check_ordinary(&std::fs::symlink_metadata(path)?, expected)?;
    opened_object_identity(path, expected)
}

fn opened_object_identity(path: &Path, expected: OrdinaryKind) -> io::Result<ObjectIdentity> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{Mode, OFlags, open};
        let mut flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW;
        if expected == OrdinaryKind::Directory {
            flags |= OFlags::DIRECTORY;
        }
        File::from(open(path, flags, Mode::empty())?)
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?
    };
    handle_object_identity(&file, expected)
}

/// Query full identity and expected ordinary kind on the same opened handle.
/// A caller-provided handle must have been opened without following the leaf
/// link if that property is required: the handle cannot reveal a followed alias.
pub fn handle_object_identity(file: &File, expected: OrdinaryKind) -> io::Result<ObjectIdentity> {
    let metadata = file.metadata()?;
    check_ordinary(&metadata, expected)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(ObjectIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
            FILE_ID_INFO, FileAttributeTagInfo, FileIdInfo, GetFileInformationByHandleEx,
        };
        let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
        // SAFETY: File owns a live handle; the initialized output has the layout
        // and size required by FileAttributeTagInfo and lives through the call.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileAttributeTagInfo,
                (&raw mut attributes).cast(),
                u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).map_err(io::Error::other)?,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0)
                != (expected == OrdinaryKind::Directory)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "opened object is not the expected ordinary file or directory",
            ));
        }
        let mut identity = FILE_ID_INFO::default();
        // SAFETY: same live handle; initialized FILE_ID_INFO storage and its
        // exact size correspond to FileIdInfo. No pointer escapes the call.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&raw mut identity).cast(),
                u32::try_from(size_of::<FILE_ID_INFO>()).map_err(io::Error::other)?,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(ObjectIdentity::Windows {
            volume_serial: identity.VolumeSerialNumber,
            file_id: identity.FileId.Identifier,
        })
    }
}

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

/// Legacy identity and hard-link count from an opened file, never synthetic IDs.
/// Windows uses a 64-bit index, which is not guaranteed unique on `ReFS`. New
/// full-width observations should use [`handle_object_identity`] instead.
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

#[cfg(test)]
mod object_identity_tests {
    use super::*;

    #[test]
    fn ordinary_objects_retain_identity_and_distinguish_replacements() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source");
        std::fs::write(&path, b"same bytes").unwrap();
        let first = object_identity(&path, OrdinaryKind::File).unwrap();
        assert_eq!(object_identity(&path, OrdinaryKind::File).unwrap(), first);
        let handle = File::open(&path).unwrap();
        assert_eq!(
            handle_object_identity(&handle, OrdinaryKind::File).unwrap(),
            first
        );
        let alias = root.path().join("alias");
        std::fs::hard_link(&path, &alias).unwrap();
        assert_eq!(object_identity(&alias, OrdinaryKind::File).unwrap(), first);
        std::fs::rename(&path, root.path().join("retained")).unwrap();
        std::fs::write(&path, b"same bytes").unwrap();
        assert_ne!(object_identity(&path, OrdinaryKind::File).unwrap(), first);
        assert_eq!(
            handle_object_identity(&handle, OrdinaryKind::File).unwrap(),
            first
        );
        let directory = root.path().join("directory");
        std::fs::create_dir(&directory).unwrap();
        let before = object_identity(&directory, OrdinaryKind::Directory).unwrap();
        assert_eq!(
            object_identity(&directory, OrdinaryKind::Directory).unwrap(),
            before
        );
        let moved = root.path().join("old-directory");
        std::fs::rename(&directory, &moved).unwrap();
        assert_eq!(
            object_identity(&moved, OrdinaryKind::Directory).unwrap(),
            before
        );
        std::fs::create_dir(&directory).unwrap();
        assert_ne!(
            object_identity(&directory, OrdinaryKind::Directory).unwrap(),
            before
        );
    }

    #[test]
    fn wrong_kind_and_missing_paths_fail() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        std::fs::write(&path, b"data").unwrap();
        assert_eq!(
            object_identity(&path, OrdinaryKind::Directory)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            object_identity(root.path(), OrdinaryKind::File)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let file = File::open(&path).unwrap();
        assert_eq!(
            handle_object_identity(&file, OrdinaryKind::Directory)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let missing =
            object_identity(&root.path().join("missing"), OrdinaryKind::File).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        assert!(missing.raw_os_error().is_some());
    }

    #[test]
    fn full_windows_identity_bits_participate_in_equality_and_serialization() {
        let base = ObjectIdentity::Windows {
            volume_serial: 0,
            file_id: [0; 16],
        };
        for bit in 0..64 {
            let changed = ObjectIdentity::Windows {
                volume_serial: 1 << bit,
                file_id: [0; 16],
            };
            assert_ne!(changed, base);
        }
        let encoded = toml::to_string(&base).unwrap();
        assert!(encoded.contains("windows"));
        for bit in 0..128 {
            let mut file_id = [0; 16];
            file_id[bit / 8] = 1 << (bit % 8);
            let changed = ObjectIdentity::Windows {
                volume_serial: 0,
                file_id,
            };
            assert_ne!(changed, base);
            assert_ne!(toml::to_string(&changed).unwrap(), encoded);
        }
        // TOML integers are signed; use a high volume bit within that range.
        let high_volume = ObjectIdentity::Windows {
            volume_serial: 1 << 48,
            file_id: [0; 16],
        };
        assert_ne!(toml::to_string(&high_volume).unwrap(), encoded);
        assert_ne!(
            base,
            ObjectIdentity::Unix {
                device: 0,
                inode: 0
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn leaf_links_and_substituted_links_are_rejected() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        std::fs::write(&target, b"untouched").unwrap();
        for (name, target, kind) in [
            ("file-link", target.clone(), OrdinaryKind::File),
            (
                "dir-link",
                root.path().to_path_buf(),
                OrdinaryKind::Directory,
            ),
            ("dangling", root.path().join("absent"), OrdinaryKind::File),
        ] {
            let link = root.path().join(name);
            symlink(target, &link).unwrap();
            assert_eq!(
                object_identity(&link, kind).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            // Opening is also safe if substitution occurs after path precheck.
            assert!(opened_object_identity(&link, kind).is_err());
        }
        let path = root.path().join("replace");
        std::fs::write(&path, b"old").unwrap();
        check_ordinary(
            &std::fs::symlink_metadata(&path).unwrap(),
            OrdinaryKind::File,
        )
        .unwrap();
        std::fs::rename(&path, root.path().join("old")).unwrap();
        symlink(&target, &path).unwrap();
        let error = opened_object_identity(&path, OrdinaryKind::File).unwrap_err();
        assert!(error.raw_os_error().is_some());
        assert_eq!(std::fs::read(target).unwrap(), b"untouched");
    }

    #[cfg(unix)]
    #[test]
    fn special_files_are_rejected_even_after_precheck() {
        use rustix::fs::{CWD, Mode, mkfifoat};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("object");
        std::fs::write(&path, b"old").unwrap();
        check_ordinary(
            &std::fs::symlink_metadata(&path).unwrap(),
            OrdinaryKind::File,
        )
        .unwrap();
        std::fs::remove_file(&path).unwrap();
        mkfifoat(CWD, &path, Mode::RUSR | Mode::WUSR).unwrap();
        assert_eq!(
            object_identity(&path, OrdinaryKind::File)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        // NONBLOCK prevents a FIFO substituted after precheck from hanging.
        assert_eq!(
            opened_object_identity(&path, OrdinaryKind::File)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let socket = root.path().join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert_eq!(
            object_identity(&socket, OrdinaryKind::File)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[cfg(unix)]
    #[test]
    fn permission_failure_retains_os_error() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("denied");
        std::fs::write(&path, b"data").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
        let result = object_identity(&path, OrdinaryKind::File);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let error = result.expect_err("run permission qualification as an ordinary user");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.raw_os_error().is_some());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "strict native Windows qualification requires junction creation"]
    fn windows_junction_is_not_an_ordinary_directory() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("data"), b"untouched").unwrap();
        let junction = root.path().join("junction");
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "required junction fixture unavailable: {output:?}"
        );
        let path_result = object_identity(&junction, OrdinaryKind::Directory);
        let handle_result = opened_object_identity(&junction, OrdinaryKind::Directory);
        std::fs::remove_dir(&junction).unwrap();
        assert_eq!(path_result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            handle_result.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(target.join("data")).unwrap(), b"untouched");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "strict native Windows qualification requires symlink privileges or Developer Mode"]
    fn windows_reparse_points_and_substitution_are_rejected() {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        std::fs::write(&target, b"untouched").unwrap();
        let file_link = root.path().join("file-link");
        symlink_file(&target, &file_link).expect("required Windows symlink fixture unavailable");
        let dir_link = root.path().join("dir-link");
        symlink_dir(root.path(), &dir_link)
            .expect("required Windows directory symlink unavailable");
        let dangling = root.path().join("dangling");
        symlink_file(root.path().join("missing"), &dangling).unwrap();
        for (path, kind) in [
            (&file_link, OrdinaryKind::File),
            (&dir_link, OrdinaryKind::Directory),
            (&dangling, OrdinaryKind::File),
        ] {
            assert_eq!(
                object_identity(path, kind).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(
                opened_object_identity(path, kind).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        let path = root.path().join("replace");
        std::fs::write(&path, b"old").unwrap();
        check_ordinary(
            &std::fs::symlink_metadata(&path).unwrap(),
            OrdinaryKind::File,
        )
        .unwrap();
        std::fs::rename(&path, root.path().join("old")).unwrap();
        symlink_file(&target, &path).unwrap();
        assert_eq!(
            opened_object_identity(&path, OrdinaryKind::File)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
        std::fs::remove_dir(dir_link).unwrap();
    }
}
