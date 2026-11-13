//! The project is the folder the user chose. Basecraft never works it out
//! from the files: a file inside the folder belongs to it, any other file is
//! a loose file.
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Clones share one use count, so whoever holds the chosen project can tell
/// whether an open session or a running job still works in it.
#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
    uses: Arc<()>,
}

impl PartialEq for Project {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}
impl Eq for Project {}

impl Project {
    /// The folder the user chose. It must exist.
    pub fn new(folder: impl AsRef<Path>) -> crate::Result<Self> {
        let root = folder.as_ref().canonicalize()?;
        if !root.is_dir() {
            return Err(crate::error::invalid("a project must be a folder"));
        }
        Ok(Self {
            root,
            uses: Arc::default(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What a file's run links are relative to: the root of `project` when it
    /// holds the file, else the file's own folder.
    pub fn base_of(project: Option<&Self>, file: &Path) -> crate::Result<PathBuf> {
        match project.filter(|project| project.contains(file)) {
            Some(project) => Ok(project.root.clone()),
            None => std::path::absolute(file)?
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| crate::error::invalid("the file has no folder")),
        }
    }

    /// Whether another clone of this project is alive, such as the project
    /// of an open session.
    pub fn in_use(&self) -> bool {
        Arc::strong_count(&self.uses) > 1
    }

    /// Whether `path` lies inside the project. A missing file counts by its
    /// folder, so a Save As destination is placed like an existing file.
    pub fn contains(&self, path: &Path) -> bool {
        self.relative(path).is_some()
    }

    fn relative(&self, path: &Path) -> Option<PathBuf> {
        let absolute = std::path::absolute(path).ok()?;
        let canonical = absolute.canonicalize().ok().or_else(|| {
            let parent = absolute.parent()?.canonicalize().ok()?;
            Some(parent.join(absolute.file_name()?))
        })?;
        canonical
            .strip_prefix(&self.root)
            .ok()
            .map(Path::to_path_buf)
    }

    /// The project on the recent-projects list `recent` that holds `file`,
    /// comparing canonical paths; the innermost one when they nest (D16).
    /// Only the list is consulted: basecraft never searches the disk for a
    /// project.
    pub fn recent_holding(file: &Path, recent: &[PathBuf]) -> Option<Self> {
        recent
            .iter()
            .filter_map(|root| Self::new(root).ok())
            .filter(|project| project.contains(file))
            .max_by_key(|project| project.root.components().count())
    }

    /// Where a result inside the project keeps its history.
    pub fn provenance_bundle(&self, result: &Path) -> Option<PathBuf> {
        let relative = self.relative(result)?;
        Some(
            self.root.join(".basecraft/provenance").join(
                basecraft_io::hash_bytes(relative.to_string_lossy().as_bytes())
                    .trim_start_matches("b3:"),
            ),
        )
    }
}

/// The recent-projects list the desktop app keeps in basecraft's private
/// folder.
pub fn recent_projects_file() -> crate::Result<PathBuf> {
    let paths = basecraft_platform::paths::AppPaths::discover()
        .map_err(|e| crate::error::invalid(e.to_string()))?;
    Ok(paths.state.join("recent-projects.json"))
}

#[cfg(test)]
mod tests {
    use super::Project;

    #[test]
    fn a_file_names_the_innermost_recent_project_that_holds_it() {
        let root = tempfile::tempdir().unwrap();
        let outer = root.path().join("outer");
        let inner = outer.join("inner");
        std::fs::create_dir_all(inner.join("data")).unwrap();
        std::fs::create_dir(root.path().join("unlisted")).unwrap();
        let file = inner.join("data/a.fasta");
        std::fs::write(&file, b">a\nA\n").unwrap();
        let recent = [outer.clone(), inner.clone(), root.path().join("gone")];
        let holder = |file: &std::path::Path, recent: &[std::path::PathBuf]| {
            Project::recent_holding(file, recent).map(|p| p.root().to_path_buf())
        };
        let inner = inner.canonicalize().unwrap();
        assert_eq!(holder(&file, &recent), Some(inner.clone()));
        // Another spelling of the same file, and a file not written yet.
        let dotted = outer.join("inner/../inner/data/a.fasta");
        assert_eq!(holder(&dotted, &recent), Some(inner.clone()));
        assert_eq!(holder(&inner.join("new.gb"), &recent), Some(inner));
        assert_eq!(
            holder(&outer.join("b.fasta"), &recent),
            Some(outer.canonicalize().unwrap())
        );
        // Only listed projects count; no folder on disk is examined.
        let loose = root.path().join("unlisted/c.fasta");
        assert_eq!(holder(&loose, &recent), None);
        assert_eq!(holder(&file, &[]), None);
    }

    #[test]
    fn membership_is_the_chosen_folder_only() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let sub = project.join("analysis/.basecraft");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(root.path().join("loose.fasta"), b">a\nA\n").unwrap();
        let project = Project::new(&project).unwrap();
        assert!(project.contains(&project.root().join("analysis/new.fasta")));
        assert!(!project.contains(&root.path().join("loose.fasta")));
        assert!(
            project
                .provenance_bundle(&root.path().join("loose.fasta"))
                .is_none()
        );
        assert!(Project::new(root.path().join("missing")).is_err());
    }
}
