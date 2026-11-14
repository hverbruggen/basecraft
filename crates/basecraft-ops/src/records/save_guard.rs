//! Shared save reservation and accepted-byte checks. Domain serializers and
//! recovery remain with their document owners. Lock order: target, then evidence.
use crate::error::{changed_on_disk, unsupported};
use std::path::{Path, PathBuf};

use crate::{Result, error::invalid};

pub(crate) type ChangeStamp = (basecraft_protocol::provenance::FileStamp, i64, i64);

pub(crate) fn change_stamp(path: &Path) -> Result<Option<ChangeStamp>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() && !metadata.file_type().is_symlink() {
        return Err(unsupported("save target must be an ordinary file"));
    }
    // One read of the file's metadata gives the stamp and the change time.
    let metadata = std::fs::metadata(path)?;
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        (metadata.ctime(), metadata.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = (0, 0);
    Ok(Some((
        crate::records::input_evidence::stamp_metadata(path, &metadata)?,
        changed.0,
        changed.1,
    )))
}

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

/// Reserve a save target in this thread's mutation scope. Other threads and
/// processes get Busy until the scope ends; it does not lock external editors.
pub(crate) fn lock(target: &Path) -> Result<crate::records::input_evidence::Locks> {
    let locks = crate::records::input_evidence::lock()?;
    locks.source(target)?;
    Ok(locks)
}

/// Test helper: another thread holds `target` until the value is dropped.
#[cfg(test)]
pub(crate) struct HeldElsewhere(
    Option<std::sync::mpsc::Sender<()>>,
    Option<std::thread::JoinHandle<()>>,
);

#[cfg(test)]
pub(crate) fn hold_elsewhere(target: &Path) -> HeldElsewhere {
    let target = target.to_path_buf();
    let (ready_tx, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let thread = std::thread::spawn(move || {
        let _held = lock(&target).unwrap();
        ready_tx.send(()).unwrap();
        let _ = wait.recv();
    });
    ready.recv().unwrap();
    HeldElsewhere(Some(release), Some(thread))
}

#[cfg(test)]
impl Drop for HeldElsewhere {
    fn drop(&mut self) {
        drop(self.0.take());
        self.1.take().unwrap().join().unwrap();
    }
}

pub(crate) fn check_hash(path: &Path, expected: &str) -> Result<()> {
    if basecraft_io::hash_file(path)? != expected {
        return Err(changed_on_disk(path));
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
        crate::isolated_profile::isolate!();
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
        crate::isolated_profile::isolate!();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gene.fasta");
        std::fs::write(&file, ">gene\nACGT\n").unwrap();
        let mut session = crate::Session::new(dir.path());
        let doc = session.open(&file).unwrap();
        let edit = Op::seq_replace(1, 2, "T");
        session.apply(doc, edit).unwrap();
        // Another saver holds the target through its whole save.
        let held = super::hold_elsewhere(&dir.path().join("./gene.fasta"));
        for target in [None, Some(file.as_path())] {
            let error = session.save(doc, target).unwrap_err();
            assert!(matches!(error, crate::Error::Busy(_)), "{error}");
        }
        let figure = super::hold_elsewhere(&dir.path().join("figure.svg"));
        let error = session
            .export(doc, &dir.path().join("figure.svg"), "svg", |w| {
                w.write_all(b"")
            })
            .unwrap_err();
        assert!(matches!(error, crate::Error::Busy(_)), "{error}");
        assert!(!dir.path().join("figure.svg").exists());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), ">gene\nACGT\n");
        assert!(session.can_undo(doc).unwrap());
        drop((held, figure));
        session.save(doc, None).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("ATGT"));
    }
}
