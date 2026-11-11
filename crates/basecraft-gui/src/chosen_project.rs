//! The project the user chose in this window: the one place that holds it.
//! Only the project picker changes it; opening a file never does. Every
//! session the window makes takes a clone of it, so its use count says
//! whether documents or jobs still work in it.
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use basecraft_ops::{Session, project::Project};

static CHOSEN: Mutex<Option<Project>> = Mutex::new(None);

fn held() -> MutexGuard<'static, Option<Project>> {
    CHOSEN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Choose `folder` (empty for none). Refused while a session or job still
/// works in the current project: its saves would go to the old project's
/// history while every reader looks in the new one. Close those documents,
/// after asking about unsaved work, and choose again.
pub(crate) fn choose(folder: &str) -> Result<(), String> {
    let next = (!folder.is_empty())
        .then(|| Project::new(folder))
        .transpose()
        .map_err(|_| format!("Project folder is unavailable: {folder}"))?;
    let mut held = held();
    if *held == next {
        return Ok(());
    }
    if let Some(current) = held.as_ref()
        && current.in_use()
    {
        return Err(format!(
            "Close the documents open in {} before choosing another project",
            current.root().display()
        ));
    }
    *held = next;
    Ok(())
}

/// The chosen project. A chosen folder that has gone missing is an error,
/// never a reason to read or write somewhere else.
pub(crate) fn chosen() -> Result<Option<Project>, String> {
    let held = held();
    match held.as_ref() {
        Some(project) if !project.root().is_dir() => Err(format!(
            "The chosen project folder is missing: {}",
            project.root().display()
        )),
        project => Ok(project.cloned()),
    }
}

/// A recoverable session for `file`, in the chosen project if there is one.
pub(crate) fn session_for(file: &Path) -> Result<Session, String> {
    let project = chosen()?;
    let root = project.as_ref().map_or_else(
        || crate::bridge::file_folder(file),
        |p| p.root().to_path_buf(),
    );
    Ok(Session::recoverable(root)
        .map_err(|e| e.to_string())?
        .in_project(project))
}

#[cfg(test)]
mod tests {
    #[test]
    fn switching_waits_until_the_old_project_is_no_longer_in_use() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_path = first.path().to_str().unwrap();
        super::choose(first_path).unwrap();
        let session = super::session_for(&first.path().join("a.fasta")).unwrap();
        let error = super::choose(second.path().to_str().unwrap()).unwrap_err();
        assert!(error.starts_with("Close the documents"), "{error}");
        // Choosing the same folder again is not a switch.
        super::choose(first_path).unwrap();
        drop(session);
        super::choose(second.path().to_str().unwrap()).unwrap();
        assert_eq!(
            super::chosen().unwrap().unwrap().root(),
            second.path().canonicalize().unwrap()
        );
        super::choose("").unwrap();
    }
}
