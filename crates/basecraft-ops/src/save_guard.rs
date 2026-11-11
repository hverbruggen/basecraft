//! Shared save reservation and accepted-byte checks. Domain serializers and
//! recovery remain with their document owners. Lock order: target, then evidence.
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

use crate::{Result, error::invalid};

pub(crate) fn target_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let parent = absolute
        .parent()
        .ok_or_else(|| invalid("save target has no parent"))?;
    Ok(parent.canonicalize()?.join(
        absolute
            .file_name()
            .ok_or_else(|| invalid("save target has no filename"))?,
    ))
}

/// Whether two spellings name the same directory entry. Parent aliases and
/// `..` resolve; a final symbolic link stays its own entry.
pub(crate) fn same_target(a: &Path, b: &Path) -> bool {
    a == b || matches!((target_path(a), target_path(b)), (Ok(a), Ok(b)) if a == b)
}

/// Cooperating processes hold this through data, binding and provenance writes.
/// The evidence lock must be acquired afterwards; neither locks external editors.
pub(crate) fn lock(target: &Path) -> Result<std::fs::File> {
    let root = lock_root();
    std::fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    let canonical = target.canonicalize().or_else(|_| target_path(target))?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(&root, &canonical))?;
    require_reservation(file.try_lock_exclusive()?)?;
    Ok(file)
}

pub(crate) fn lock_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "basecraft-save-locks-{}",
        basecraft_io::hash_bytes(basecraft_platform::username().as_bytes())
            .trim_start_matches("b3:")
    ))
}
pub(crate) fn lock_path(root: &Path, canonical: &Path) -> PathBuf {
    let digest = basecraft_io::hash_bytes(canonical.as_os_str().as_encoded_bytes());
    root.join(format!("{}.lock", digest.trim_start_matches("b3:")))
}
pub(crate) fn require_reservation(acquired: bool) -> Result<()> {
    if acquired {
        Ok(())
    } else {
        Err(invalid(
            "another Basecraft process is saving this target; retry shortly",
        ))
    }
}

#[allow(dead_code)]
pub struct Reservation(crate::service_reads::ServiceLockGuard);
#[allow(dead_code)]
impl Reservation {
    pub fn reauthorize(&self, access: &dyn crate::access::Access) -> Result<()> {
        self.0.reauthorize(access)
    }
}
#[allow(dead_code)]
pub(crate) fn lock_with_access(
    context: std::sync::Arc<crate::service_reads::ServiceContext>,
    access: &dyn crate::access::Access,
    target: &Path,
    purpose: crate::service_reads::SaveTargetPurpose,
) -> Result<Reservation> {
    crate::service_reads::acquire_lock(context, access, Some((target, purpose))).map(Reservation)
}

pub(crate) fn check_hash(path: &Path, expected: &str) -> Result<()> {
    if basecraft_io::hash_file(path)? != expected {
        return Err(crate::Error::ChangedOnDisk {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// Refuse aliases and existing files at a new review destination.
pub(crate) fn reject_link_target(target: &Path) -> Result<()> {
    // Replacing a link atomically would split its identity and support binding.
    if let Ok(metadata) = std::fs::symlink_metadata(target) {
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                "save target is a symbolic link; choose a direct new destination",
            ));
        }
        if basecraft_platform::files::link_count(target)? > 1 {
            return Err(invalid(
                "save target has hard links; choose a new destination",
            ));
        }
    }
    Ok(())
}

pub(crate) fn check_new_target(target: &Path) -> Result<()> {
    reject_link_target(target)?;
    if target_path(target)?.try_exists()? {
        return Err(invalid(
            "Save As target already exists; choose a new path or reopen it",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use basecraft_protocol::Op;

    #[test]
    fn hard_link_destination_is_rejected_on_every_platform() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let alias = directory.path().join("alias");
        std::fs::write(&source, b"preserve").unwrap();
        std::fs::hard_link(&source, &alias).unwrap();
        let error = super::check_new_target(&source).unwrap_err();
        assert!(error.to_string().contains("hard links"));
        assert_eq!(std::fs::read(&alias).unwrap(), b"preserve");
    }

    #[test]
    fn concurrent_saver_is_refused_and_edits_remain_saveable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gene.fasta");
        std::fs::write(&file, ">gene\nACGT\n").unwrap();
        let mut session = crate::Session::new(dir.path());
        let doc = session.open(&file).unwrap();
        let edit = Op::SeqReplace {
            start: 1,
            end: 2,
            residues: "T".into(),
        };
        session.apply(doc, edit).unwrap();
        // Another Basecraft process holds the target through its whole save.
        let held = super::lock(&dir.path().join("./gene.fasta")).unwrap();
        for target in [None, Some(file.as_path())] {
            let error = session.save(doc, target).unwrap_err().to_string();
            assert!(error.contains("another Basecraft process"), "{error}");
        }
        let figure = super::lock(&dir.path().join("figure.svg")).unwrap();
        let error = session
            .export(doc, &dir.path().join("figure.svg"), "svg", |w| {
                w.write_all(b"")
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("another Basecraft process"), "{error}");
        assert!(!dir.path().join("figure.svg").exists());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), ">gene\nACGT\n");
        assert!(session.can_undo(doc).unwrap());
        drop((held, figure));
        session.save(doc, None).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("ATGT"));
    }
}
