//! The project the user chose in this window. Only the project picker sets it;
//! opening a file never does. Every session the window makes takes it from here.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use basecraft_ops::{Session, project::Project};

static CHOSEN: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Remember the folder the user chose; an empty path means none.
pub(crate) fn choose(folder: &str) {
    let folder = (!folder.is_empty()).then(|| PathBuf::from(folder));
    *CHOSEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = folder;
}

/// The chosen project. A chosen folder that has gone missing is an error,
/// never a reason to read or write somewhere else.
pub(crate) fn chosen() -> Result<Option<Project>, String> {
    let folder = CHOSEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    folder
        .map(|folder| {
            Project::new(&folder)
                .map_err(|_| format!("The chosen project folder is missing: {}", folder.display()))
        })
        .transpose()
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
