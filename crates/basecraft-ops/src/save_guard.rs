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
    let root = std::env::temp_dir().join(format!(
        "basecraft-save-locks-{}",
        basecraft_io::hash_bytes(basecraft_platform::username().as_bytes())
            .trim_start_matches("b3:")
    ));
    std::fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    let canonical = target.canonicalize().or_else(|_| target_path(target))?;
    let digest = basecraft_io::hash_bytes(canonical.as_os_str().as_encoded_bytes());
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(format!("{}.lock", digest.trim_start_matches("b3:"))))?;
    file.try_lock_exclusive()
        .map_err(|_| invalid("another Basecraft process is saving this target; retry shortly"))?;
    Ok(file)
}

pub(crate) fn check_hash(path: &Path, expected: &str) -> Result<()> {
    if basecraft_io::hash_file(path)? != expected {
        return Err(invalid(
            "source changed on disk; save to a new path or reopen it",
        ));
    }
    Ok(())
}

/// Transient authority for replacing one accepted review destination.
#[derive(Clone)]
pub(crate) struct Revision {
    path: PathBuf,
    hash: String,
    stamp: basecraft_protocol::provenance::FileStamp,
}

impl Revision {
    pub(crate) fn accepted(path: &Path, bytes: &[u8]) -> Result<Self> {
        let revision = Self {
            path: target_path(path)?,
            hash: basecraft_io::hash_bytes(bytes),
            stamp: crate::input_evidence::stamp(path)?,
        };
        check_hash(path, &revision.hash)?;
        Ok(revision)
    }

    pub(crate) fn check(expected: Option<&Self>, target: &Path) -> Result<bool> {
        // Replacing a link atomically would split its identity and support binding.
        if let Ok(metadata) = std::fs::symlink_metadata(target) {
            if metadata.file_type().is_symlink() {
                return Err(invalid(
                    "save target is a symbolic link; choose a direct new destination",
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() > 1 {
                    return Err(invalid(
                        "save target has hard links; choose a new destination",
                    ));
                }
            }
        }
        let target = target_path(target)?;
        if let Some(expected) = expected.filter(|e| e.path == target) {
            check_hash(&target, &expected.hash).map_err(|error| {
                invalid(format!(
                    "save target changed or is unavailable: {error}; choose a new path or reopen it"
                ))
            })?;
            if crate::input_evidence::stamp(&target)? != expected.stamp {
                return Err(invalid(
                    "save target was replaced or modified; save to a new path or reopen it",
                ));
            }
            return Ok(true);
        } else if target.try_exists()? {
            return Err(invalid(
                "Save As target already exists; choose a new path or reopen it",
            ));
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use basecraft_protocol::Op;
    use std::io::Write;

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
            .export(doc, &dir.path().join("figure.svg"), "svg", |w| w.write_all(b""))
            .unwrap_err()
            .to_string();
        assert!(error.contains("another Basecraft process"), "{error}");
        assert!(!dir.path().join("figure.svg").exists());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), ">gene\nACGT\n");
        assert!(session.can_undo(doc));
        drop((held, figure));
        session.save(doc, None).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("ATGT"));
    }
}
